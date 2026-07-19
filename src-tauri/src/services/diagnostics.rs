use crate::{
    api::error::{PublicError, PublicErrorCode},
    app::AppServices,
    security::redact::redact_text,
};

pub fn build_diagnostics(services: &AppServices) -> String {
    let snapshot = services.snapshot();
    let last_success = snapshot
        .last_success_at
        .map(|value| value.to_rfc3339())
        .unwrap_or_else(|| "--".to_string());
    let last_error = snapshot
        .last_error
        .as_ref()
        .map(diagnostic_error_label)
        .unwrap_or_else(|| "--".to_string());
    let text = format!(
        "SevnX Monitor {}\n系统：{}\n登录状态：{:?}\n最后成功刷新：{}\n最近公开错误：{}\n日志目录：{}",
        env!("CARGO_PKG_VERSION"),
        std::env::var("OS").unwrap_or_else(|_| "Windows".to_string()),
        snapshot.auth,
        last_success,
        last_error,
        services.paths().logs_dir().display(),
    );
    redact_text(&text)
}

/// Diagnostics deliberately show only the enum category. Although current UI
/// messages are generic, this avoids turning a future server- or user-derived
/// `PublicError::message` into a diagnostic disclosure path.
fn diagnostic_error_label(error: &PublicError) -> String {
    format!("{:?}", error.code)
}

pub fn copy_text_to_clipboard(text: &str) -> Result<(), PublicError> {
    #[cfg(windows)]
    {
        use std::ptr::copy_nonoverlapping;
        use windows::Win32::{
            Foundation::{GlobalFree, HANDLE},
            System::{
                DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
                Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock},
                Ole::CF_UNICODETEXT,
            },
        };

        struct ClipboardGuard;
        impl Drop for ClipboardGuard {
            fn drop(&mut self) {
                unsafe {
                    let _ = CloseClipboard();
                }
            }
        }

        let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            OpenClipboard(None).map_err(|_| clipboard_error())?;
        }
        let _guard = ClipboardGuard;
        unsafe {
            EmptyClipboard().map_err(|_| clipboard_error())?;
            let memory = GlobalAlloc(GMEM_MOVEABLE, utf16.len() * std::mem::size_of::<u16>())
                .map_err(|_| clipboard_error())?;
            let pointer = GlobalLock(memory).cast::<u16>();
            if pointer.is_null() {
                let _ = GlobalFree(Some(memory));
                return Err(clipboard_error());
            }
            copy_nonoverlapping(utf16.as_ptr(), pointer, utf16.len());
            let _ = GlobalUnlock(memory);
            if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0))).is_err() {
                let _ = GlobalFree(Some(memory));
                return Err(clipboard_error());
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = text;
        Err(clipboard_error())
    }
}

fn clipboard_error() -> PublicError {
    PublicError::new(PublicErrorCode::Internal, "无法写入系统剪贴板")
}

#[cfg(test)]
mod tests {
    use crate::api::error::{PublicError, PublicErrorCode};

    use super::{diagnostic_error_label, redact_text};

    #[test]
    fn diagnostic_redaction_removes_embedded_test_credentials() {
        let text =
            redact_text("最近错误：token=test-diagnostic-token; Cookie: session=test-cookie");
        assert!(!text.contains("test-diagnostic-token"));
        assert!(!text.contains("test-cookie"));
    }

    #[test]
    fn diagnostic_error_summary_never_includes_a_public_error_message() {
        let label = diagnostic_error_label(&PublicError::new(
            PublicErrorCode::Internal,
            "token=test-diagnostic-token",
        ));
        assert_eq!(label, "Internal");
        assert!(!label.contains("test-diagnostic-token"));
    }
}
