//! Creates desktop / start-menu shortcuts that launch Codex with the
//! debug port and relay the `sevnx://relaunch?dbg=PORT` argument to SevnX so
//! the overlay is injected (design §9 / §13).
//!
//! The shortcut target is the running `sevnx-monitor.exe`; its Arguments are
//! the relaunch URL. Clicking it starts/focuses SevnX and injects the bar.
//! The icon is taken from the installed Codex (Store package assets or the
//! standalone executable) so the shortcut keeps the original Codex look.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use windows::core::{Interface, PCWSTR};
use windows::Win32::{
    System::Com::{
        CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
        CoTaskMemFree, CoUninitialize, IPersistFile,
    },
    UI::Shell::{
        FOLDERID_Desktop, FOLDERID_Programs, IShellLinkW, KF_FLAG_DEFAULT, SHGetKnownFolderPath,
        ShellLink,
    },
};

/// The Codex CDP debug port used by the relaunch URL and the shortcut.
pub const CODEX_DEBUG_PORT: u16 = 9229;

/// Where to place the shortcut.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShortcutLocation {
    Desktop,
    StartMenu,
}

/// One-shot COM apartment for the current thread (must outlive the COM calls).
struct ComApartment;

impl ComApartment {
    fn init() -> Result<Self, String> {
        unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED)
                .ok()
                .map_err(|error| error.to_string())?;
        }
        Ok(Self)
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        unsafe {
            CoUninitialize();
        }
    }
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.to_string_lossy().encode_utf16().chain(std::iter::once(0)).collect()
}

fn folder_for(location: ShortcutLocation) -> Option<PathBuf> {
    let id = match location {
        ShortcutLocation::Desktop => &FOLDERID_Desktop,
        ShortcutLocation::StartMenu => &FOLDERID_Programs,
    };
    unsafe {
        let item = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let value = item.to_string().ok().map(PathBuf::from);
        CoTaskMemFree(Some(item.as_ptr().cast()));
        value
    }
}

/// OpenAI desktop package family names (identity + publisher id).
const OPENAI_PACKAGE_FAMILIES: &[&str] = &[
    "OpenAI.Codex_2p2nqsd0c76g0",
    "OpenAI.CodexBeta_2p2nqsd0c76g0",
    "OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0",
];

#[cfg(windows)]
fn store_codex_install_dir() -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::{
        APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
    };
    use windows::Win32::Storage::Packaging::Appx::{
        GetPackagePathByFullName, GetPackagesByPackageFamily,
    };
    use windows::core::{PCWSTR, PWSTR};

    for family in OPENAI_PACKAGE_FAMILIES {
        let family = family.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
        let mut count = 0u32;
        let mut buffer_length = 0u32;
        let first = unsafe {
            GetPackagesByPackageFamily(
                PCWSTR(family.as_ptr()),
                &mut count,
                None,
                &mut buffer_length,
                None,
            )
        };
        if first == APPMODEL_ERROR_NO_PACKAGE || (first == ERROR_SUCCESS && count == 0) {
            continue;
        }
        if first != ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        let mut pointers = vec![PWSTR(std::ptr::null_mut()); count as usize];
        let mut buffer = vec![0u16; buffer_length as usize];
        let status = unsafe {
            GetPackagesByPackageFamily(
                PCWSTR(family.as_ptr()),
                &mut count,
                Some(pointers.as_mut_ptr()),
                &mut buffer_length,
                Some(PWSTR(buffer.as_mut_ptr())),
            )
        };
        if status != ERROR_SUCCESS {
            continue;
        }
        buffer.truncate(buffer_length as usize);
        let full_names = buffer
            .split(|value| *value == 0)
            .filter(|value| !value.is_empty())
            .map(|value| String::from_utf16(value).unwrap_or_default())
            .collect::<Vec<_>>();
        for full_name in full_names {
            let full_name = full_name.encode_utf16().chain(std::iter::once(0)).collect::<Vec<_>>();
            let mut path_length = 0u32;
            if unsafe {
                GetPackagePathByFullName(PCWSTR(full_name.as_ptr()), &mut path_length, None)
            } != ERROR_INSUFFICIENT_BUFFER
            {
                continue;
            }
            let mut path = vec![0u16; path_length as usize];
            if unsafe {
                GetPackagePathByFullName(
                    PCWSTR(full_name.as_ptr()),
                    &mut path_length,
                    Some(PWSTR(path.as_mut_ptr())),
                )
            } != ERROR_SUCCESS
            {
                continue;
            }
            let end = path.iter().position(|value| *value == 0).unwrap_or(path.len());
            return Some(PathBuf::from(OsString::from_wide(&path[..end])));
        }
    }
    None
}

#[cfg(not(windows))]
fn store_codex_install_dir() -> Option<PathBuf> {
    None
}

/// Wraps a PNG payload in an ICO container (Vista+ recognizes PNG-compressed
/// icons). Width/height are read from the PNG IHDR.
fn png_to_ico(png: &[u8]) -> Option<Vec<u8>> {
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" {
        return None;
    }
    let width = u32::from_be_bytes([png[16], png[17], png[18], png[19]]);
    let height = u32::from_be_bytes([png[20], png[21], png[22], png[23]]);
    let mut ico = Vec::with_capacity(22 + png.len());
    // ICONDIR
    ico.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
    // ICONDIRENTRY
    ico.push(if width >= 256 { 0 } else { width as u8 });
    ico.push(if height >= 256 { 0 } else { height as u8 });
    ico.extend_from_slice(&[0, 0, 1, 0, 32, 0]);
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(png);
    Some(ico)
}

/// Resolves a Codex icon that can be passed to `SetIconLocation`.
/// Returns `(icon_path, prefer_default_index)`.
fn codex_icon_source() -> Option<PathBuf> {
    // Prefer the Store package's Square44x44 logo (crisp, genuine Codex icon).
    if let Some(dir) = store_codex_install_dir() {
        let assets = dir.join("Assets");
        for name in [
            "icon.png",
            "Square44x44Logo.png",
            "Square44x44Logo.targetsize-256_altform-unplated.png",
        ] {
            let source = assets.join(name);
            let Ok(bytes) = std::fs::read(&source) else {
                continue;
            };
            let Some(ico) = png_to_ico(&bytes) else {
                continue;
            };
            let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)?;
            let target = local.join("SevnX Monitor").join("codex.ico");
            if std::fs::write(&target, ico).is_ok() {
                return Some(target);
            }
        }
    }
    // Fall back to a standalone executable we can point at directly.
    standalone_codex_exe()
}

/// Locates a standalone Codex desktop executable (for icon fallback).
fn standalone_codex_exe() -> Option<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA")?;
    let roots = [
        PathBuf::from(&local).join("OpenAI").join("Codex"),
        PathBuf::from(&local).join("Programs").join("OpenAI").join("Codex"),
    ];
    for name in ["Codex.exe", "ChatGPT.exe", "codex.exe"] {
        for root in &roots {
            let direct = root.join(name);
            if direct.is_file() {
                return Some(direct);
            }
            let bin = root.join("bin");
            if bin.is_dir() {
                if let Some(candidate) = recursive_exe_in(&bin, name) {
                    return Some(candidate);
                }
            }
        }
    }
    None
}

fn recursive_exe_in(dir: &Path, name: &str) -> Option<PathBuf> {
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let candidate = path.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Creates `<location>/Codex (SevnX 监控).lnk` pointing at this exe with the
/// `sevnx://relaunch?dbg=PORT` argument. Returns the shortcut path.
pub fn create_codex_shortcut(location: ShortcutLocation) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let folder = folder_for(location).ok_or_else(|| "无法定位目标文件夹".to_string())?;
    let shortcut_path = folder.join("Codex (SevnX 监控).lnk");

    let _com = ComApartment::init().map_err(|error| error.to_string())?;
    unsafe {
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| error.to_string())?;
        shell_link
            .SetPath(PCWSTR(wide_path(&exe).as_ptr()))
            .map_err(|error| error.to_string())?;
        let arguments = format!("sevnx://relaunch?dbg={CODEX_DEBUG_PORT}");
        shell_link
            .SetArguments(PCWSTR(wide(&arguments).as_ptr()))
            .map_err(|error| error.to_string())?;
        if let Some(directory) = exe.parent() {
            shell_link
                .SetWorkingDirectory(PCWSTR(wide_path(directory).as_ptr()))
                .map_err(|error| error.to_string())?;
        }
        shell_link
            .SetDescription(PCWSTR(wide("启动 Codex 并注入 SevnX 余额横条").as_ptr()))
            .map_err(|error| error.to_string())?;
        if let Some(icon) = codex_icon_source() {
            shell_link
                .SetIconLocation(PCWSTR(wide_path(&icon).as_ptr()), 0)
                .map_err(|error| error.to_string())?;
        }

        let persist_file: IPersistFile = shell_link.cast().map_err(|error| error.to_string())?;
        persist_file
            .Save(PCWSTR(wide_path(&shortcut_path).as_ptr()), true)
            .map_err(|error| error.to_string())?;
    }
    Ok(shortcut_path)
}
