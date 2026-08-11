//! Windows-only self-registration of the `sevnx://` custom URI protocol.
//!
//! Runs at every startup (idempotent) so the overlay's "reconnect SevnX"
//! action works on any machine without needing installer-side registry edits
//! (design §13 / §17). Registering on each launch also keeps the command path
//! pointing at the current install location after an update.

use std::path::Path;

use windows::core::PCWSTR;
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
    REG_VALUE_TYPE, RegCloseKey,
    RegCreateKeyExW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

const PROTOCOL_CLASS: &str = "Software\\Classes\\sevnx";
const COMMAND_SUBKEY: &str = "Software\\Classes\\sevnx\\shell\\open\\command";

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Registers `sevnx://` user-level (HKCU) so no elevation is required.
pub fn register_sevnx_protocol(exe_path: &Path) -> Result<(), String> {
    create_class_key()?;
    let expected_command = protocol_command(exe_path);
    create_command_key(&expected_command)?;
    let actual_command = read_registered_command()?;
    if actual_command != expected_command {
        return Err("registered sevnx protocol command does not match current executable".to_string());
    }
    Ok(())
}

fn protocol_command(exe_path: &Path) -> String {
    format!("\"{}\" \"%1\"", exe_path.display())
}

fn create_class_key() -> Result<(), String> {
    let path = wide(PROTOCOL_CLASS);
    let mut key: HKEY = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if status.0 != 0 {
        return Err(format!("RegCreateKeyExW({PROTOCOL_CLASS}) failed: {}", status.0));
    }
    let result = set_value(key, "", "URL:sevnx Protocol")
        .and_then(|_| set_value(key, "URL Protocol", ""));
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}

fn create_command_key(command: &str) -> Result<(), String> {
    let path = wide(COMMAND_SUBKEY);
    let mut key: HKEY = HKEY::default();
    let status = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        )
    };
    if status.0 != 0 {
        return Err(format!("RegCreateKeyExW({COMMAND_SUBKEY}) failed: {}", status.0));
    }
    let result = set_value(key, "", command);
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}

fn read_registered_command() -> Result<String, String> {
    let path = wide(COMMAND_SUBKEY);
    let mut key: HKEY = HKEY::default();
    let status = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            None,
            KEY_READ,
            &mut key,
        )
    };
    if status.0 != 0 {
        return Err(format!("RegOpenKeyExW({COMMAND_SUBKEY}) failed: {}", status.0));
    }
    let result = (|| {
        let name = wide("");
        let mut value_type = REG_VALUE_TYPE(0);
        let mut byte_count = 0u32;
        let status = unsafe {
            RegQueryValueExW(
                key,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut value_type),
                None,
                Some(&mut byte_count),
            )
        };
        if status.0 != 0 {
            return Err(format!("RegQueryValueExW({COMMAND_SUBKEY}) failed: {}", status.0));
        }
        if value_type != REG_SZ || byte_count == 0 {
            return Err("registered sevnx protocol command has an invalid type".to_string());
        }
        let mut buffer = vec![0u16; (byte_count as usize).div_ceil(2)];
        let status = unsafe {
            RegQueryValueExW(
                key,
                PCWSTR(name.as_ptr()),
                None,
                Some(&mut value_type),
                Some(buffer.as_mut_ptr().cast()),
                Some(&mut byte_count),
            )
        };
        if status.0 != 0 {
            return Err(format!("RegQueryValueExW({COMMAND_SUBKEY}) failed: {}", status.0));
        }
        let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
        String::from_utf16(&buffer[..end])
            .map_err(|_| "registered sevnx protocol command is not UTF-16".to_string())
    })();
    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}

fn set_value(key: HKEY, name: &str, text: &str) -> Result<(), String> {
    let name_wide = wide(name);
    let data = wide(text);
    let data_bytes = unsafe {
        std::slice::from_raw_parts(data.as_ptr().cast::<u8>(), data.len() * 2)
    };
    let status = unsafe {
        RegSetValueExW(
            key,
            PCWSTR(name_wide.as_ptr()),
            None,
            REG_SZ,
            Some(data_bytes),
        )
    };
    if status.0 != 0 {
        Err(format!("RegSetValueExW({name}) failed: {}", status.0))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::protocol_command;

    #[test]
    fn protocol_command_quotes_executable_and_uri_argument() {
        assert_eq!(
            protocol_command(Path::new(r"C:\Program Files\SevnX\sevnx-monitor.exe")),
            r#""C:\Program Files\SevnX\sevnx-monitor.exe" "%1""#,
        );
    }
}
