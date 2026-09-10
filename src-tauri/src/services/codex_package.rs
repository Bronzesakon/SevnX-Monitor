//! Discovers the installed Codex desktop host and how to launch it.
//!
//! Ported from Codex++ v1.3.0 (`crates/codex-plus-core/src/app_paths.rs`).
//! Two upstream findings matter for recent Codex releases:
//!
//! 1. Codex now ships as the `OpenAI.ChatGPT-Desktop` package, whose MSIX full
//!    name carries a `~` resource id — for example
//!    `OpenAI.ChatGPT-Desktop_2026.514.421.0_neutral_~_2p2nqsd0c76g0`. Splitting
//!    the publisher with `rsplit_once("__")` cannot read that form, so the
//!    package looked absent and the app appeared "not installed".
//! 2. Package *registration* is authoritative, not the `WindowsApps` directory:
//!    a Store update changes both the full name and the install directory, so
//!    neither is cached across launches.
//!
//! Everything here is read-only discovery; launching lives in [`super::codex_launcher`].

use std::path::{Path, PathBuf};

/// A supported OpenAI desktop package identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PackageSpec {
    identity: &'static str,
    app_id: &'static str,
    priority: u8,
}

const PACKAGE_SPECS: &[PackageSpec] = &[
    PackageSpec {
        identity: "OpenAI.Codex",
        app_id: "App",
        priority: 1,
    },
    PackageSpec {
        identity: "OpenAI.CodexBeta",
        app_id: "App",
        priority: 1,
    },
    // Codex now ships as the ChatGPT desktop host; prefer it when both exist.
    PackageSpec {
        identity: "OpenAI.ChatGPT-Desktop",
        app_id: "App",
        priority: 2,
    },
];

/// Family name = identity + publisher id. The version and architecture appear
/// only in the full name, so the family survives Store updates.
#[cfg(windows)]
const PACKAGE_FAMILY_NAMES: &[&str] = &[
    "OpenAI.Codex_2p2nqsd0c76g0",
    "OpenAI.CodexBeta_2p2nqsd0c76g0",
    "OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0",
];

/// `ChatGPT.exe` is the current Electron host, `Codex.exe` the older one.
const EXECUTABLE_NAMES: &[&str] = &["ChatGPT.exe", "Codex.exe", "codex.exe"];

/// Resolves the Codex app directory.
///
/// Registered MSIX packages win because a Store update moves the install
/// directory; only when registration is unavailable does this fall back to
/// scanning `WindowsApps` and then to a standalone install.
pub fn resolve_app_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        if let Some(app_dir) = find_registered_app_dir() {
            return Some(app_dir);
        }
        if let Some(app_dir) = windows_app_roots()
            .iter()
            .filter_map(|root| find_latest_in_dir(root))
            .max_by(compare_app_dirs)
        {
            return Some(app_dir);
        }
    }
    find_standalone_app_dir()
}

/// Derives the AppUserModelID from the package directory name, e.g.
/// `…\OpenAI.ChatGPT-Desktop_2026.514.421.0_neutral_~_2p2nqsd0c76g0\app` becomes
/// `OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0!App`.
///
/// Returns `None` for a standalone install, which is spawned rather than
/// activated.
pub fn packaged_aumid(app_dir: &Path) -> Option<String> {
    let package_name = package_name_from_app_dir(app_dir)?;
    let (spec, _, publisher_id) = package_parts(&package_name)?;
    if publisher_id.is_empty() {
        return None;
    }
    Some(format!("{}_{publisher_id}!{}", spec.identity, spec.app_id))
}

/// Absolute path of the executable to spawn for a standalone install.
pub fn executable_for(app_dir: &Path) -> PathBuf {
    executable_in_dir(app_dir).unwrap_or_else(|| app_dir.join(EXECUTABLE_NAMES[0]))
}

/// The package directory behind a resolved app dir (`…\Pkg\app` → `…\Pkg`).
/// Store assets (icons) live here, not in the `app` subdirectory.
pub fn package_root(app_dir: &Path) -> &Path {
    if is_app_component(app_dir) {
        app_dir.parent().unwrap_or(app_dir)
    } else {
        app_dir
    }
}

/// True when `path` lies inside an OpenAI Store package directory. Used to tell
/// the packaged Codex host apart from a plain standalone ChatGPT install.
pub fn is_store_package_path(path: &Path) -> bool {
    path.ancestors()
        .any(|ancestor| spec_from_dir(ancestor).is_some())
}

/// True when a process is a Codex desktop host.
///
/// `Codex.exe` is accepted anywhere. `ChatGPT.exe` is accepted only from a Store
/// package path: a standalone ChatGPT install is a plain chat app that must not
/// be treated as Codex or terminated along with it.
pub fn is_codex_host(exe_file: &str, exe_path: Option<&Path>) -> bool {
    if exe_file.eq_ignore_ascii_case("Codex.exe") {
        return true;
    }
    exe_file.eq_ignore_ascii_case("ChatGPT.exe") && exe_path.is_some_and(is_store_package_path)
}

/// Locates a standalone (non-Store) Codex installation.
pub fn find_standalone_app_dir() -> Option<PathBuf> {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA")?);
    let roots = [
        local.join("OpenAI").join("ChatGPT"),
        local.join("Programs").join("OpenAI").join("ChatGPT"),
        local.join("OpenAI").join("Codex"),
        local.join("Programs").join("OpenAI").join("Codex"),
    ];
    roots.into_iter().find_map(|root| standalone_dir_in(&root))
}

/// Searches a standalone root, then `bin`, then the versioned `bin/<version>`
/// directories the standalone installer creates.
fn standalone_dir_in(root: &Path) -> Option<PathBuf> {
    if executable_in_dir(root).is_some() {
        return Some(root.to_path_buf());
    }
    let bin = root.join("bin");
    if executable_in_dir(&bin).is_some() {
        return Some(bin);
    }
    std::fs::read_dir(&bin)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .find(|path| executable_in_dir(path).is_some())
}

fn executable_in_dir(dir: &Path) -> Option<PathBuf> {
    EXECUTABLE_NAMES
        .iter()
        .map(|name| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Resolves a raw path (package install location, saved setting, or user
/// selection) into a Codex app directory.
fn normalize_app_dir(path: &Path) -> Option<PathBuf> {
    if path.as_os_str().is_empty() {
        return None;
    }
    if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
        if name.eq_ignore_ascii_case("Codex.exe") || name.eq_ignore_ascii_case("ChatGPT.exe") {
            return path.parent().map(Path::to_path_buf);
        }
    }
    if executable_in_dir(path).is_some() {
        return Some(path.to_path_buf());
    }
    let nested = path.join("app");
    if nested.is_dir() && (executable_in_dir(&nested).is_some() || is_store_package_path(path)) {
        return Some(nested);
    }
    // WindowsApps ACLs often hide the executables; the package name is enough.
    if path.is_dir() && is_store_package_path(path) {
        return Some(path.to_path_buf());
    }
    None
}

/// The newest app dir in a `WindowsApps`-style root, preferring the highest
/// package priority and then the highest version.
fn find_latest_in_dir(root: &Path) -> Option<PathBuf> {
    std::fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .filter_map(|package_dir| {
            let spec = spec_from_dir(&package_dir)?;
            let version = version_tuple(&package_dir)?;
            let app_dir = package_entry_dir(&package_dir)?;
            Some((spec.priority, version, app_dir))
        })
        .max_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)))
        .map(|(_, _, app_dir)| app_dir)
}

fn package_entry_dir(package_dir: &Path) -> Option<PathBuf> {
    let app = package_dir.join("app");
    if app.is_dir() {
        return Some(app);
    }
    if executable_in_dir(package_dir).is_some() {
        return Some(package_dir.to_path_buf());
    }
    None
}

fn compare_app_dirs(left: &PathBuf, right: &PathBuf) -> std::cmp::Ordering {
    app_dir_sort_key(left).cmp(&app_dir_sort_key(right))
}

/// `(priority, version)`: higher priority wins, then the newest version, so a
/// Store update is picked up on the next launch.
fn app_dir_sort_key(app_dir: &Path) -> Option<(u8, Vec<u32>)> {
    let spec = spec_from_dir(app_dir)?;
    let package_dir = if is_app_component(app_dir) {
        app_dir.parent()?
    } else {
        app_dir
    };
    Some((spec.priority, version_tuple(package_dir)?))
}

fn is_app_component(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case("app"))
}

fn spec_from_dir(path: &Path) -> Option<PackageSpec> {
    let package_name = package_name_from_app_dir(path)?;
    let (spec, _, _) = package_parts(&package_name)?;
    Some(spec)
}

/// The MSIX package name for a path, ignoring a trailing `app` component.
fn package_name_from_app_dir(app_dir: &Path) -> Option<String> {
    let path = app_dir.to_string_lossy().replace('\\', "/");
    let mut parts = path.split('/').filter(|part| !part.is_empty());
    let mut name = parts.next_back()?.to_string();
    if name.eq_ignore_ascii_case("app") {
        name = parts.next_back()?.to_string();
    }
    Some(name)
}

fn version_tuple(package_dir: &Path) -> Option<Vec<u32>> {
    let name = package_dir.file_name()?.to_str()?;
    let (_, version, _) = package_parts(name)?;
    let parts = version
        .split('.')
        .map(str::parse::<u32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (!parts.is_empty()).then_some(parts)
}

/// Splits an MSIX package name into `(spec, version, publisher id)`.
///
/// Accepts both the classic `Name_1.2.3.0_x64__publisher` and the newer
/// `Name_1.2.3.0_neutral_~_publisher`, where the resource id is `~`.
fn package_parts(package_name: &str) -> Option<(PackageSpec, &str, &str)> {
    for spec in PACKAGE_SPECS {
        let Some(rest) = strip_prefix_ignore_ascii_case(package_name, spec.identity) else {
            continue;
        };
        let Some(rest) = rest.strip_prefix('_') else {
            continue;
        };
        let Some((version, rest)) = rest.split_once('_') else {
            continue;
        };
        let Some((_, publisher_id)) = rest.rsplit_once('_') else {
            continue;
        };
        if publisher_id.is_empty() {
            continue;
        }
        return Some((*spec, version, publisher_id));
    }
    None
}

fn strip_prefix_ignore_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let head = value.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

#[cfg(windows)]
fn windows_app_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        roots.push(PathBuf::from(program_files).join("WindowsApps"));
    }
    if let Some(program_files) = std::env::var_os("ProgramW6432") {
        roots.push(PathBuf::from(program_files).join("WindowsApps"));
    }
    roots.push(PathBuf::from(r"C:\Program Files\WindowsApps"));
    roots.sort();
    roots.dedup();
    roots
}

/// App dir of the highest-priority registered OpenAI package.
///
/// Queried fresh on every launch: a Store update changes both the full name and
/// the install directory, so a cached answer would point at a stale package.
#[cfg(windows)]
fn find_registered_app_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();
    for family_name in PACKAGE_FAMILY_NAMES {
        let Ok(full_names) = package_full_names_for_family(family_name) else {
            continue; // family not registered, or the packaging API is unavailable
        };
        for full_name in full_names {
            let Ok(install_location) = package_path_by_full_name(&full_name) else {
                continue;
            };
            if let Some(app_dir) = normalize_app_dir(&install_location) {
                candidates.push(app_dir);
            }
        }
    }
    candidates.into_iter().max_by(compare_app_dirs)
}

/// Full package names (name + version + arch + resource id + publisher) for a
/// package family, via `GetPackagesByPackageFamily`.
#[cfg(windows)]
fn package_full_names_for_family(family_name: &str) -> Result<Vec<String>, String> {
    use windows::Win32::Foundation::{
        APPMODEL_ERROR_NO_PACKAGE, ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS,
    };
    use windows::Win32::Storage::Packaging::Appx::GetPackagesByPackageFamily;
    use windows::core::{PCWSTR, PWSTR};

    let family = family_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
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
        return Ok(Vec::new());
    }
    if first != ERROR_INSUFFICIENT_BUFFER {
        return Err(format!(
            "GetPackagesByPackageFamily failed with {}",
            first.0
        ));
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
        return Err(format!(
            "GetPackagesByPackageFamily failed with {}",
            status.0
        ));
    }
    buffer.truncate(buffer_length as usize);
    Ok(buffer
        .split(|value| *value == 0)
        .filter(|value| !value.is_empty())
        .map(|value| String::from_utf16_lossy(value))
        .collect())
}

#[cfg(windows)]
fn package_path_by_full_name(full_name: &str) -> Result<PathBuf, String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
    use windows::Win32::Storage::Packaging::Appx::GetPackagePathByFullName;
    use windows::core::{PCWSTR, PWSTR};

    let full_name = full_name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let mut path_length = 0u32;
    let first =
        unsafe { GetPackagePathByFullName(PCWSTR(full_name.as_ptr()), &mut path_length, None) };
    if first != ERROR_INSUFFICIENT_BUFFER {
        return Err(format!("GetPackagePathByFullName failed with {}", first.0));
    }
    let mut path = vec![0u16; path_length as usize];
    let status = unsafe {
        GetPackagePathByFullName(
            PCWSTR(full_name.as_ptr()),
            &mut path_length,
            Some(PWSTR(path.as_mut_ptr())),
        )
    };
    if status != ERROR_SUCCESS {
        return Err(format!("GetPackagePathByFullName failed with {}", status.0));
    }
    let end = path
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(path.len());
    Ok(PathBuf::from(OsString::from_wide(&path[..end])))
}

#[cfg(test)]
mod tests {
    use super::{is_codex_host, package_parts, packaged_aumid};
    use std::path::{Path, PathBuf};

    fn aumid(package_dir: &str) -> Option<String> {
        packaged_aumid(&PathBuf::from(format!(
            r"C:\Program Files\WindowsApps\{package_dir}\app"
        )))
    }

    #[test]
    fn parses_chatgpt_desktop_package_with_tilde_resource_id() {
        // The form Codex moved to; the previous `rsplit_once("__")` parse missed it.
        assert_eq!(
            aumid("OpenAI.ChatGPT-Desktop_2026.514.421.0_neutral_~_2p2nqsd0c76g0").as_deref(),
            Some("OpenAI.ChatGPT-Desktop_2p2nqsd0c76g0!App")
        );
    }

    #[test]
    fn parses_classic_package_name() {
        assert_eq!(
            aumid("OpenAI.Codex_26.707.3748.0_x64__2p2nqsd0c76g0").as_deref(),
            Some("OpenAI.Codex_2p2nqsd0c76g0!App")
        );
        assert_eq!(
            aumid("OpenAI.CodexBeta_26.527.7698.0_x64__2p2nqsd0c76g0").as_deref(),
            Some("OpenAI.CodexBeta_2p2nqsd0c76g0!App")
        );
    }

    #[test]
    fn non_desktop_chatgpt_package_is_ignored() {
        // `OpenAI.CodexBeta` must not be matched by the `OpenAI.Codex` prefix.
        assert!(package_parts("OpenAI.CodexBeta_26.527.7698.0_x64__abc").is_some());
        assert!(package_parts("OpenAI.ChatGPT_1.0.0.0_x64__abc").is_none());
        assert!(aumid("OpenAI.ChatGPT_1.0.0.0_x64__abc").is_none());
    }

    #[test]
    fn standalone_install_has_no_aumid() {
        assert_eq!(aumid("Codex.exe"), None);
        assert_eq!(
            packaged_aumid(Path::new(r"C:\Users\me\AppData\Local\OpenAI\Codex\bin")),
            None
        );
    }

    #[test]
    fn chatgpt_host_counts_only_from_a_store_package() {
        let store = PathBuf::from(
            r"C:\Program Files\WindowsApps\OpenAI.ChatGPT-Desktop_2026.514.421.0_neutral_~_abc\app\ChatGPT.exe",
        );
        let standalone = PathBuf::from(r"C:\Users\me\AppData\Local\Programs\ChatGPT\ChatGPT.exe");
        assert!(is_codex_host("ChatGPT.exe", Some(&store)));
        assert!(!is_codex_host("ChatGPT.exe", Some(&standalone)));
        assert!(!is_codex_host("ChatGPT.exe", None));
        // The older standalone host is Codex by name alone.
        assert!(is_codex_host("Codex.exe", Some(&standalone)));
        assert!(is_codex_host("Codex.exe", None));
        assert!(!is_codex_host("chrome.exe", Some(&store)));
    }
}
