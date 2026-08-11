//! Launches the installed Codex desktop app with a CDP debug port and injects
//! the SevnX overlay (design §13).
//!
//! Codex ships in two forms on Windows:
//!   - Microsoft Store package (e.g. `OpenAI.Codex_...`), activated via COM
//!     `IApplicationActivationManager` because its exe lives in an ACL-walled
//!     WindowsApps directory and must be launched with arguments.
//!   - Standalone installer, spawned directly as a detached process.
//!
//! Flow:
//!   1. If Codex is already reachable on the debug port → inject directly.
//!   2. Otherwise try Store activation first, then fall back to the standalone
//!      executable, both with `--remote-debugging-port` + `--remote-allow-origins`.
//!   3. Poll the debug endpoint until reachable, then inject (silent retries).
//!   4. Start the watchdog to keep the overlay alive across reloads.
//!
//! Only loopback is ever touched; the port is fixed and validated at the CDP layer.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant as StdInstant};

use tauri::Manager;

use crate::app::AppServices;
use crate::services::codex_inject;
use crate::services::shortcut::CODEX_DEBUG_PORT;

/// How long to wait for the Codex debug endpoint to come up after launch.
const BOOT_TIMEOUT: Duration = Duration::from_secs(45);
const RETRY_INTERVAL: Duration = Duration::from_millis(500);

/// Publisher ID shared by the OpenAI Codex / ChatGPT desktop packages.
const OPENAI_PUBLISHER_ID: &str = "2p2nqsd0c76g0";

/// Entry point for the `launch_codex` command. Returns the user-facing notice
/// text: whether Codex was already running (inject-only / blocked) or freshly
/// launched, so the frontend can toast the accurate message.
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
    // Codex is running but without a debug port (non-debug session). Activating
    // it would only focus the existing window and never let us inject, so we
    // must not launch another instance — just surface a top notice.
    if codex_process_running() {
        services
            .logger()
            .write_dynamic("codex_already_running", "reason=no_debug_port".to_string());
        return Ok("检测到 Codex 正在运行（未开启调试端口），未另外拉起。请先手动退出 Codex，再点击「打开带状态窗的 Codex」。".to_string());
    }

    let arguments = format!(
        "--remote-debugging-port={debug_port} \
         --remote-allow-origins=http://127.0.0.1:{debug_port}"
    );

    // Prefer the Store package (what most users have), fall back to standalone.
    let mut launched = false;
    for aumid in packaged_codex_aumids() {
        match activate_packaged(&aumid, &arguments).await {
            Ok(pid) => {
                services.logger().write_dynamic(
                    "codex_activated",
                    format!("result=ok aumid={aumid} pid={pid}"),
                );
                launched = true;
                break;
            }
            Err(_) => continue, // package not installed or activation refused
        }
    }
    if !launched {
        let Some(exe) = detect_codex_executable() else {
            let message = "未找到 Codex，请安装 Codex 桌面版".to_string();
            services
                .logger()
                .write_dynamic("codex_launch_failed", format!("reason=exe_not_found"));
            return Err(message);
        };
        if let Err(error) = spawn_codex(&exe, debug_port) {
            services
                .logger()
                .write_dynamic("codex_launch_failed", format!("reason=spawn_failed error={error}"));
            return Err(error);
        }
        services.logger().write_dynamic(
            "codex_launched",
            format!("result=standalone debug_port={debug_port} exe={}", exe.display()),
        );
    }

    if let Err(error) = wait_and_inject(&services, debug_port).await {
        services.logger().write_dynamic(
            "codex_launch_failed",
            format!("reason=inject_failed error={error}"),
        );
        return Err(error);
    }
    // Keep the overlay alive across Codex reloads.
    codex_inject::spawn_overlay_watchdog(app.clone(), debug_port);
    services.logger().write_dynamic(
        "codex_launched",
        format!("result=injected debug_port={debug_port}"),
    );
    Ok("已启动 Codex 并注入状态横条".to_string())
}

/// Candidate AppUserModelIDs for the various OpenAI desktop packages.
fn packaged_codex_aumids() -> Vec<String> {
    ["OpenAI.Codex", "OpenAI.CodexBeta", "OpenAI.ChatGPT-Desktop"]
        .iter()
        .map(|identity| format!("{identity}_{OPENAI_PUBLISHER_ID}!App"))
        .collect()
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
        coinit.ok().or_else(|error| {
            const RPC_E_CHANGED_MODE: i32 = -2147417850;
            if error.code().0 == RPC_E_CHANGED_MODE {
                Ok(())
            } else {
                Err(error)
            }
        }).map_err(|error| error.to_string())?;

        let result = (|| {
            let manager: IApplicationActivationManager = CoCreateInstance(
                &ApplicationActivationManager,
                None,
                CLSCTX_LOCAL_SERVER,
            )
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

/// Codex executable names that represent the desktop app (standalone form).
const CODEX_EXECUTABLE_NAMES: &[&str] = &["Codex.exe", "ChatGPT.exe", "codex.exe"];

/// Detects the standalone Codex desktop executable from common install roots.
fn detect_codex_executable() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let roots = [
        Path::new(&local).join("OpenAI").join("Codex"),
        Path::new(&local).join("Programs").join("OpenAI").join("Codex"),
    ];
    for root in roots {
        if let Some(exe) = standalone_exe_in(&root) {
            return Some(exe);
        }
    }
    None
}

/// Searches a Codex root for the desktop executable, including the
/// versioned sub-directories used by the standalone installer:
/// `bin/Codex.exe`, `bin/<version>/codex.exe`, `Codex.exe`, ...
fn standalone_exe_in(root: &Path) -> Option<PathBuf> {
    for name in CODEX_EXECUTABLE_NAMES {
        let direct = root.join(name);
        if direct.is_file() {
            return Some(direct);
        }
    }
    let bin = root.join("bin");
    if !bin.is_dir() {
        return None;
    }
    for name in CODEX_EXECUTABLE_NAMES {
        let direct = bin.join(name);
        if direct.is_file() {
            return Some(direct);
        }
    }
    let Ok(entries) = std::fs::read_dir(&bin) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        for name in CODEX_EXECUTABLE_NAMES {
            let candidate = path.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Spawns the standalone Codex detached so killing SevnX doesn't take Codex
/// down with it. `--remote-allow-origins` is required by recent Electron builds
/// or the CDP WebSocket handshake is rejected (mirrors Codex++).
fn spawn_codex(exe: &Path, debug_port: u16) -> Result<(), String> {
    let mut command = Command::new(exe);
    command.arg(format!("--remote-debugging-port={debug_port}"));
    command.arg(format!("--remote-allow-origins=http://127.0.0.1:{debug_port}"));
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
/// error is enriched with whether the Codex process is actually alive.
async fn wait_and_inject(services: &AppServices, debug_port: u16) -> Result<(), String> {
    let deadline = StdInstant::now() + BOOT_TIMEOUT;
    loop {
        match codex_inject::inject_overlay(debug_port).await {
            Ok(()) => {
                services.logger().write("codex_inject_ok", "result=injected");
                let payload = services.overlay_push_json();
                let _ = codex_inject::push_overlay_data(debug_port, &payload).await;
                return Ok(());
            }
            Err(_error) if StdInstant::now() < deadline => {
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
            Err(error) => {
                return Err(if codex_process_running() {
                    format!("Codex 已启动但 CDP 调试端口未就绪: {error}")
                } else {
                    format!("Codex 未能成功启动: {error}")
                });
            }
        }
    }
}

/// True if a Codex desktop process is running. Matches case-insensitively so
/// both the standalone `Codex.exe` and the Store package's `codex.exe` main
/// process are caught.
#[cfg(windows)]
fn codex_process_running() -> bool {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return false;
        };
        let mut entry = PROCESSENTRY32W::default();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut found = false;
        let mut has_next = Process32FirstW(snapshot, &mut entry).is_ok();
        while has_next {
            let name =
                String::from_utf16_lossy(&entry.szExeFile).trim_end_matches('\0').to_string();
            if name.eq_ignore_ascii_case("Codex.exe") || name.eq_ignore_ascii_case("ChatGPT.exe")
            {
                found = true;
                break;
            }
            has_next = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        found
    }
}

#[cfg(not(windows))]
fn codex_process_running() -> bool {
    false
}
