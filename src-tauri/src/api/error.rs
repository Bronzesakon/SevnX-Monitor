use serde::Serialize;
use thiserror::Error;

/// Errors that may be shown in the application UI or emitted through IPC.
/// They are intentionally generic so a response body, cookie, token, or
/// account identifier can never reach diagnostics or the Vue layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PublicErrorCode {
    Network,
    Timeout,
    AuthenticationRequired,
    PermissionDenied,
    RateLimited,
    ServiceUnavailable,
    InvalidResponse,
    BusinessRejected,
    NotReady,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicError {
    pub code: PublicErrorCode,
    pub message: String,
}

impl PublicError {
    pub fn new(code: PublicErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn not_ready() -> Self {
        Self::new(PublicErrorCode::NotReady, "该功能尚未就绪")
    }
}

/// Internal, non-serializable request/parse failure. No variant carries a raw
/// body or response message because either can contain sensitive values.
#[derive(Debug, Error)]
pub enum ApiError {
    #[error("network request failed")]
    Network,
    #[error("request timed out")]
    Timeout,
    #[error("authentication required")]
    Unauthorized,
    #[error("permission denied")]
    Forbidden,
    #[error("request rate limited")]
    RateLimited,
    #[error("service unavailable")]
    ServiceUnavailable,
    #[error("unexpected HTTP status")]
    HttpStatus,
    #[error("business response rejected")]
    BusinessRejected { code: i64 },
    #[error("invalid response schema")]
    InvalidResponse { endpoint: &'static str },
    #[error("session credentials are unavailable")]
    MissingCredentials,
}

impl ApiError {
    pub fn from_http_status(status: u16) -> Self {
        match status {
            401 => Self::Unauthorized,
            403 => Self::Forbidden,
            429 => Self::RateLimited,
            500..=599 => Self::ServiceUnavailable,
            _ => Self::HttpStatus,
        }
    }

    /// The documented invalidation signals. Network/timeout errors must never
    /// be interpreted as an expired login.
    pub fn is_auth_invalid(&self) -> bool {
        matches!(
            self,
            Self::Unauthorized | Self::Forbidden | Self::BusinessRejected { code: 401 | 403 }
        )
    }

    pub fn to_public(&self) -> PublicError {
        let (code, message) = match self {
            Self::Network => (PublicErrorCode::Network, "网络连接失败，请稍后重试"),
            Self::Timeout => (PublicErrorCode::Timeout, "请求超时，请稍后重试"),
            Self::Unauthorized | Self::MissingCredentials => (
                PublicErrorCode::AuthenticationRequired,
                "登录状态已失效，请重新登录",
            ),
            Self::Forbidden => (PublicErrorCode::PermissionDenied, "当前登录没有访问权限"),
            Self::RateLimited => (PublicErrorCode::RateLimited, "请求过于频繁，请稍后重试"),
            Self::ServiceUnavailable => (
                PublicErrorCode::ServiceUnavailable,
                "SevnX 服务暂时不可用，请稍后重试",
            ),
            Self::HttpStatus | Self::BusinessRejected { .. } => (
                PublicErrorCode::BusinessRejected,
                "SevnX 暂时无法处理该请求",
            ),
            Self::InvalidResponse { .. } => (
                PublicErrorCode::InvalidResponse,
                "SevnX 返回的数据格式无法识别",
            ),
        };
        PublicError::new(code, message)
    }
}
