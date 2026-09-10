//! Launches the installed Codex desktop host with a CDP debug port and injects
//! the SevnX overlay (design §13, §18).
//!
//! Codex ships in two forms on Windows:
//!   - Microsoft Store / MSIX package — now `OpenAI.ChatGPT-Desktop_*` — activated
//!     via COM `IApplicationActivationManager` because its exe lives in an
//!     ACL-walled WindowsApps directory and must be launched with arguments.
//!   - Standalone installer, spawned directly as a detached process.
//!
//! Flow:
//!   1. If Codex is already reachable on the debug port → inject directly.
//!   2. Otherwise resolve the host (registered package first, then standalone)
//!      and activate/spawn it with `--remote-debugging-port`.
//!   3. Poll the debug endpoint until reachable, then inject.
//!   4. If the host was *already* running, activation only brings the existing
//!      window forward and the Chromium switches never apply. In that case the
//!      host is restarted once and the launch retried.
//!   5. Start the watchdog to keep the overlay alive across reloads.
//!
//! Only loopback is ever touched; the port is fixed and validated at the CDP layer.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant as StdInstant};

use tauri::Manager;

use crate::app::AppServices;
use crate::services::codex_inject;
use crate::services::codex_package;
use crate::services::shortcut::CODEX_DEBUG_PORT;

/// How long to wait for the Codex debug endpoint to come up after launch.
const BOOT_TIMEOUT: Duration = Duration::from_secs(45);
const RETRY_INTERVAL: Duration = Duration::from_millis(500);
/// Shorter first-attempt budget when a host was already running: activation
/// either reuses it (the port appears at once) or cannot apply the Chromium
/// switches at all, so the full cold-start budget would only delay the restart.
const HOST_REUSE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long to wait for a restarted host to release its process list.
const HOST_EXIT_TIMEOUT: Duration = Duration::from_secs(15);

/// Entry point for the `launch_codex` command. Returns the user-facing notice
/// text describing what actually happened, so the frontend can toast accurately.
pub async fn launch_and_inject(app: &tauri::AppHandle) -> Result<String, String> {
    launch_and_inject_on_port(app, CODEX_DEBUG_PORT).await
}

/// Handles a `sevnx://relaunch` activation through the same lifecycle as the
/// in-app launch command.
pub async fn relaunch_and_inject(
    app: &tauri::AppHandle,
    debug_port: u16,
) -> Result<String, String> {
    launch_and_inject_on_port(app, debug_port).await
}

async fn launch_and_inject_on_port(
    app: &tauri::AppHandle,
    debug_port: u16,
) -> Result<String, String> {
    let services = app.state::<AppServices>();

    // Already running with the debug port open → inject only, never relaunch.
    if codex_inject::endpoint_available(debug_port) {
        services.inject_codex_overlay(debug_port).await?;
        codex_inject::spawn_overlay_watchdog(app.clone(), debug_port);
        return Ok("Codex 已在运行，已注入状态横条".to_string());
    }

    let target = LaunchTarget::resolve()?;
    // A host that is already running cannot pick up Chromium switches from a
    // second activation, so remember whether a restart may be required. Recent
    // Codex builds ship as the ChatGPT desktop app, which is usually running.
    let host_running = codex_host_running();

    target.launch(&services, debug_port).await?;

    let first_attempt_timeout = if host_running {
        HOST_REUSE_TIMEOUT
    } else {
        BOOT_TIMEOUT
    };
    match wait_and_inject(&services, debug_port, first_attempt_timeout).await {
        Ok(()) => {
            codex_inject::spawn_overlay_watchdog(app.clone(), debug_port);
            services.logger().write_dynamic(
                "codex_launched",
                format!("result=injected debug_port={debug_port}"),
            );
            Ok(if host_running {
                "Codex 已在运行，已重新拉起并注入状态横条".to_string()
            } else {
                "已启动 Codex 并注入状态横条".to_string()
            })
        }
        Err(error) if host_running => {
            // Activation of an already-running packaged app only focuses the
            // existing window, so the debug switches are dropped. Restart the
            // host once to let Chromium start with them.
            services.logger().write_dynamic(
                "codex_restart",
                format!("reason=debug_port_unavailable error={error}"),
            );
            restart_codex_host(&services).await;
            target.launch(&services, debug_port).await?;
            wait_and_inject(&services, debug_port, BOOT_TIMEOUT).await?;
            codex_inject::spawn_overlay_watchdog(app.clone(), debug_port);
            services.logger().write_dynamic(
                "codex_launched",
                format!("result=injected_after_restart debug_port={debug_port}"),
            );
            Ok("已重启 Codex 并注入状态横条".to_string())
        }
        Err(error) => {
            services.logger().write_dynamic(
                "codex_launch_failed",
                format!("reason=inject_failed error={error}"),
            );
            Err(error)
        }
    }
}

/// Something that can be launched with the debug arguments.
enum LaunchTarget {
    /// MSIX/Store package: activated by AppUserModelID so it starts with arguments.
    Packaged { aumid: String },
    /// Standalone install: spawned directly.
    Standalone { exe: PathBuf },
}

impl LaunchTarget {
    fn resolve() -> Result<Self, String> {
        let app_dir = codex_package::resolve_app_dir()
            .ok_or_else(|| "未找到 Codex，请安装 Codex 桌面版".to_string())?;
        if let Some(aumid) = codex_package::packaged_aumid(&app_dir) {
            return Ok(Self::Packaged { aumid });
        }
        let exe = codex_package::executable_for(&app_dir);
        if !exe.is_file() {
            return Err(format!("未找到 Codex 可执行文件: {}", exe.display()));
        }
        Ok(Self::Standalone { exe })
    }

    async fn launch(&self, services: &AppServices, debug_port: u16) -> Result<(), String> {
        let arguments = debug_arguments(debug_port);
        match self {
            Self::Packaged { aumid } => match activate_packaged(aumid, &arguments).await {
                Ok(process_id) => {
                    services.logger().write_dynamic(
                        "codex_activated",
                        format!("result=ok aumid={aumid} pid={process_id}"),
                    );
                    Ok(())
                }
                Err(error) => {
                    services.logger().write_dynamic(
                        "codex_launch_failed",
                        format!("reason=activate_failed aumid={aumid} error={error}"),
                    );
                    Err(format!("无法启动 Codex: {error}"))
                }
            },
            Self::Standalone { exe } => {
                if let Err(error) = spawn_codex(exe, debug_port) {
                    services.logger().write_dynamic(
                        "codex_launch_failed",
                        format!("reason=spawn_failed error={error}"),
                    );
                    return Err(error);
                }
                services.logger().write_dynamic(
                    "codex_launched",
                    format!(
                        "result=standalone debug_port={debug_port} exe={}",
                        exe.display()
                    ),
                );
                Ok(())
            }
        }
    }
}

/// Chromium switches Codex must start with. `--remote-allow-origins` is required
/// by recent Electron builds or the CDP WebSocket handshake is rejected.
fn debug_arguments(debug_port: u16) -> String {
    format!(
        "--remote-debugging-port={debug_port} \
         --remote-allow-origins=http://127.0.0.1:{debug_port}"
    )
}

/// Activates a Store-packaged Codex via `IApplicationActivationManager`,
/// passing the debug-port arguments. Returns the child process id.
#[cfg(windows)]
async fn activate_packaged(aumid: &str, arguments: &str) -> Result<u32, String> {
    let aumid = aumid.to_string();
    let arguments = arguments.to_string();
    tokio::task::spawn_blocking(move || activate_packaged_blocking(&aumid, &arguments))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(windows)]
fn activate_packaged_blocking(aumid: &str, arguments: &str) -> Result<u32, String> {
    use windows::Win32::System::Com::{
        CLSCTX_LOCAL_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoUninitialize,
    };
    use windows::Win32::UI::Shell::{
        ACTIVATEOPTIONS, ApplicationActivationManager, IApplicationActivationManager,
    };
    use windows::core::HSTRING;

    unsafe {
        let coinit = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let should_uninitialize = coinit.is_ok();
        coinit
            .ok()
            .or_else(|error| {
                const RPC_E_CHANGED_MODE: i32 = -2147417850;
                if error.code().0 == RPC_E_CHANGED_MODE {
                    Ok(())
                } else {
                    Err(error)
                }
            })
            .map_err(|error| error.to_string())?;

        let result = (|| {
            let manager: IApplicationActivationManager =
                CoCreateInstance(&ApplicationActivationManager, None, CLSCTX_LOCAL_SERVER)
                    .map_err(|error| error.to_string())?;
            manager
                .ActivateApplication(
                    &HSTRING::from(aumid),
                    &HSTRING::from(arguments),
                    ACTIVATEOPTIONS(0),
                )
                .map_err(|error| error.to_string())
        })();

        if should_uninitialize {
            CoUninitialize();
        }
        result
    }
}

#[cfg(not(windows))]
async fn activate_packaged(_aumid: &str, _arguments: &str) -> Result<u32, String> {
    Err("packaged activation is only supported on Windows".to_string())
}

/// Spawns a standalone Codex detached so killing SevnX doesn't take Codex
/// down with it.
fn spawn_codex(exe: &Path, debug_port: u16) -> Result<(), String> {
    let mut command = Command::new(exe);
    command.arg(format!("--remote-debugging-port={debug_port}"));
    command.arg(format!(
        "--remote-allow-origins=http://127.0.0.1:{debug_port}"
    ));
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| format!("无法启动 Codex: {error}"))
}

/// Polls the CDP endpoint until Codex is injectable, then injects and pushes
/// the first snapshot so the bar shows data without waiting for the watchdog.
/// Retries are silent so the boot window doesn't spam the log. On timeout the
/// error is enriched with whether the Codex host is actually alive.
async fn wait_and_inject(
    services: &AppServices,
    debug_port: u16,
    timeout: Duration,
) -> Result<(), String> {
    let deadline = StdInstant::now() + timeout;
    loop {
        match codex_inject::inject_overlay(debug_port).await {
            Ok(()) => {
                services
                    .logger()
                    .write("codex_inject_ok", "result=injected");
                let payload = services.overlay_push_json();
                let _ = codex_inject::push_overlay_data(debug_port, &payload).await;
                return Ok(());
            }
            Err(_error) if StdInstant::now() < deadline => {
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
            Err(error) => {
                return Err(if codex_host_running() {
                    format!("Codex 已启动但 CDP 调试端口未就绪: {error}")
                } else {
                    format!("Codex 未能成功启动: {error}")
                });
            }
        }
    }
}

/// Restarts the Codex host so the debug switches take effect. Only processes
/// recognised by [`codex_package::is_codex_host`] are touched, so an unrelated
/// standalone ChatGPT install is never closed.
async fn restart_codex_host(services: &AppServices) {
    let process_ids = codex_host_processes();
    services.logger().write_dynamic(
        "codex_restart_begin",
        format!("process_count={}", process_ids.len()),
    );
    terminate_processes(&process_ids);
    let deadline = StdInstant::now() + HOST_EXIT_TIMEOUT;
    while StdInstant::now() < deadline && codex_host_running() {
        tokio::time::sleep(RETRY_INTERVAL).await;
    }
}

/// True if a Codex desktop host process is running (Store package or standalone).
fn codex_host_running() -> bool {
    !codex_host_processes().is_empty()
}

/// Process ids of the running Codex host, matched by name and — for
/// `ChatGPT.exe` — by executable path so a plain ChatGPT install is excluded.
#[cfg(windows)]
fn codex_host_processes() -> Vec<u32> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return Vec::new();
        };
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut process_ids = Vec::new();
        let mut has_next = Process32FirstW(snapshot, &mut entry).is_ok();
        while has_next {
            let name = String::from_utf16_lossy(&entry.szExeFile)
                .trim_end_matches('\0')
                .to_string();
            // Resolving the image path costs a handle, so only do it for the one
            // name whose classification depends on it.
            let path = if name.eq_ignore_ascii_case("ChatGPT.exe") {
                process_image_path(entry.th32ProcessID)
            } else {
                None
            };
            if codex_package::is_codex_host(&name, path.as_deref()) {
                process_ids.push(entry.th32ProcessID);
            }
            has_next = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        process_ids
    }
}

#[cfg(not(windows))]
fn codex_host_processes() -> Vec<u32> {
    Vec::new()
}

#[cfg(windows)]
fn process_image_path(process_id: u32) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows::core::PWSTR;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id).ok()?;
        if handle.is_invalid() {
            return None;
        }
        let mut buffer = vec![0u16; 32 * 1024];
        let mut length = buffer.len() as u32;
        let queried = QueryFullProcessImageNameW(
            handle,
            Default::default(),
            PWSTR(buffer.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(handle);
        queried.ok()?;
        Some(PathBuf::from(OsString::from_wide(
            &buffer[..length as usize],
        )))
    }
}

#[cfg(windows)]
fn terminate_processes(process_ids: &[u32]) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};

    for process_id in process_ids {
        unsafe {
            let Ok(handle) = OpenProcess(PROCESS_TERMINATE, false, *process_id) else {
                continue;
            };
            if handle.is_invalid() {
                continue;
            }
            let _ = TerminateProcess(handle, 0);
            let _ = CloseHandle(handle);
        }
    }
}

#[cfg(not(windows))]
fn terminate_processes(_process_ids: &[u32]) {}
