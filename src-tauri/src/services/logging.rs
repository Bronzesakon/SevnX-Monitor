use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use chrono::Local;

use crate::security::redact::redact_text;

const MAX_LOG_BYTES: u64 = 512 * 1024;
const RETAINED_FILES: u8 = 3;

#[derive(Clone, Copy, Debug)]
pub(crate) enum LogLevel {
    Info,
    Error,
    Critical,
}

impl LogLevel {
    fn as_str(self) -> &'static str {
        match self {
            LogLevel::Info => "INFO",
            LogLevel::Error => "ERROR",
            LogLevel::Critical => "CRITICAL",
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SafeLog {
    directory: PathBuf,
}

impl SafeLog {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self { directory }
    }

    /// Writes fixed, non-sensitive telemetry labels and runs a second
    /// redaction pass as defense in depth. Requiring static labels prevents
    /// callers from accidentally sending an API body, request header, cookie,
    /// token, or account field to the log sink.
    pub(crate) fn write(&self, event: &'static str, detail: &'static str) {
        self.write_line(LogLevel::Info, event, detail);
    }

    pub(crate) fn write_critical(&self, event: &'static str, detail: &'static str) {
        self.write_line(LogLevel::Critical, event, detail);
    }

    pub(crate) fn write_error(&self, event: &'static str, detail: &'static str) {
        self.write_line(LogLevel::Error, event, detail);
    }

    /// Same as [`write`](Self::write) but accepts a caller-formatted detail
    /// string. Only for non-sensitive metadata (timestamps, remaining seconds)
    /// that SafeLog itself formats; the redaction pass still applies.
    pub(crate) fn write_dynamic(&self, event: &'static str, detail: String) {
        self.write_line(LogLevel::Info, event, &detail);
    }

    pub(crate) fn write_login_api_path(&self, path: &str) {
        if path.len() > 160
            || !path.starts_with("/api/")
            || !path.chars().all(|character| {
                character.is_ascii_alphanumeric() || matches!(character, '/' | '-' | '_')
            })
        {
            self.write("login_request_path_rejected", "reason=invalid_path");
            return;
        }
        self.write_line(LogLevel::Info, "login_request_path", &format!("api_path={path}"));
    }

    fn write_line(&self, level: LogLevel, event: &'static str, detail: &str) {
        let _ = fs::create_dir_all(&self.directory);
        let path = self.directory.join("sevnx-monitor.log");
        rotate_if_needed(&path);
        let safe_line = redact_text(&format!(
            "{} [{}] event={} detail={}\n",
            Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
            level.as_str(),
            event,
            detail,
        ));
        if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
            let _ = file.write_all(safe_line.as_bytes());
        }
        #[cfg(debug_assertions)]
        {
            eprint!("{safe_line}");
        }
    }
}

fn rotate_if_needed(path: &Path) {
    let Ok(metadata) = fs::metadata(path) else {
        return;
    };
    if metadata.len() < MAX_LOG_BYTES {
        return;
    }
    for index in (1..=RETAINED_FILES).rev() {
        let source = if index == 1 {
            path.to_path_buf()
        } else {
            path.with_extension(format!("log.{}", index - 1))
        };
        let target = path.with_extension(format!("log.{index}"));
        if source.exists() {
            let _ = fs::rename(source, target);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::SafeLog;

    #[test]
    fn logs_redact_test_credential_values() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("sevnx-log-{stamp}"));
        let log = SafeLog::new(directory.clone());
        log.write("refresh", "Authorization: Bearer test-log-token");
        let content = fs::read_to_string(directory.join("sevnx-monitor.log")).expect("log");
        assert!(!content.contains("test-log-token"));
        fs::remove_dir_all(directory).expect("cleanup");
    }

    #[test]
    fn logs_never_keep_cookie_or_token_key_values() {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("sevnx-log-cookie-{stamp}"));
        let log = SafeLog::new(directory.clone());
        log.write(
            "refresh",
            "cookie=test-log-cookie; access_token=test-log-access-token",
        );
        let content = fs::read_to_string(directory.join("sevnx-monitor.log")).expect("log");
        assert!(!content.contains("test-log-cookie"));
        assert!(!content.contains("test-log-access-token"));
        fs::remove_dir_all(directory).expect("cleanup");
    }
}
