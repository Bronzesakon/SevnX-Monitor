use std::{
    collections::BTreeMap,
    sync::Arc,
    time::{Duration, SystemTime},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::{api::error::ApiError, model::AuthStatus};

/// Safe session metadata only. Credentials are deliberately absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionStatus {
    pub auth: AuthStatus,
    pub last_validated_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Default)]
pub struct Session {
    inner: Arc<RwLock<SessionInner>>,
}

#[derive(Default)]
struct SessionInner {
    auth: AuthStatus,
    credentials: Option<PersistedSession>,
    /// A re-login must not destroy a still-valid session when the user closes
    /// the WebView2 window before validation finishes.
    login_backup: Option<PersistedSession>,
    last_validated_at: Option<DateTime<Utc>>,
    /// Monotonically invalidates WebView2 candidates when a login window is
    /// closed or a newer login attempt supersedes it. A cookie read before a
    /// close must never be committed afterwards.
    login_generation: u64,
}

/// Kept crate-private and intentionally not `Debug`. It is serialized only as
/// input to DPAPI, never to an IPC response, log, or diagnostic summary.
#[derive(Clone, Default, Deserialize, Serialize)]
pub(crate) struct RequestContext {
    #[serde(default)]
    pub(crate) api_origin: Option<String>,
    #[serde(default)]
    pub(crate) user_agent: Option<String>,
    #[serde(default)]
    pub(crate) accept: Option<String>,
    #[serde(default)]
    pub(crate) accept_language: Option<String>,
    #[serde(default)]
    pub(crate) sec_fetch_dest: Option<String>,
    #[serde(default)]
    pub(crate) sec_fetch_mode: Option<String>,
    #[serde(default)]
    pub(crate) sec_fetch_site: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct PersistedSession {
    cookie_header: Option<String>,
    #[serde(default, alias = "bearer_token")]
    authorization_header: Option<String>,
    /// 用于 /auth/refresh 自动续期。仅保存在 DPAPI 密文内，绝不对外暴露。
    #[serde(default)]
    refresh_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
    last_validated_at: Option<DateTime<Utc>>,
    #[serde(default)]
    request_context: RequestContext,
}

/// Only crate code that builds an HTTP request may observe this value.
/// It must never be made public, serialized, formatted, or logged.
pub(crate) struct RequestCredentials {
    pub(crate) cookie_header: Option<String>,
    pub(crate) authorization_header: Option<String>,
    pub(crate) refresh_token: Option<String>,
    pub(crate) request_context: RequestContext,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn status(&self) -> SessionStatus {
        let inner = self.inner.read().await;
        SessionStatus {
            auth: inner.auth,
            last_validated_at: inner.last_validated_at,
            expires_at: inner
                .credentials
                .as_ref()
                .and_then(|credentials| credentials.expires_at),
        }
    }

    /// 是否已持有 refresh_token（仅返回布尔，绝不暴露值）。同步版本供诊断摘要使用。
    pub(crate) fn has_refresh_token(&self) -> bool {
        self.inner
            .try_read()
            .ok()
            .is_some_and(|inner| {
                inner
                    .credentials
                    .as_ref()
                    .is_some_and(|credentials| credentials.refresh_token.is_some())
            })
    }

    pub async fn begin_login(&self) -> u64 {
        let mut inner = self.inner.write().await;
        // A second click while the same login window is open must preserve the
        // original backup rather than replacing it with unvalidated cookies.
        if inner.login_backup.is_none() {
            inner.login_backup = inner.credentials.clone();
        }
        inner.login_generation = inner.login_generation.wrapping_add(1);
        inner.auth = AuthStatus::LoggingIn;
        inner.login_generation
    }

    pub(crate) async fn login_generation(&self) -> u64 {
        self.inner.read().await.login_generation
    }

    pub async fn begin_validation(&self) {
        self.inner.write().await.auth = AuthStatus::Validating;
    }

    /// 仅供测试使用：构造"已持凭据但在 Validating 中间态"的状态，用来验证登录
    /// 取消/回滚逻辑。生产登录提交走 [`commit_verified_login`](Self::commit_verified_login)，
    /// 本方法不被生产代码调用。crate-private 防止 IPC 调用方提供或读取凭据。
    #[allow(dead_code)]
    pub(crate) async fn install_credentials(
        &self,
        cookie_header: Option<String>,
        authorization_header: Option<String>,
        refresh_token: Option<String>,
        expires_at: Option<DateTime<Utc>>,
        request_context: RequestContext,
    ) {
        let mut inner = self.inner.write().await;
        inner.credentials = Some(PersistedSession {
            cookie_header,
            authorization_header: normalize_authorization_header(authorization_header),
            refresh_token,
            expires_at,
            last_validated_at: None,
            request_context,
        });
        inner.auth = AuthStatus::Validating;
    }

    /// Commits a WebView2 cookie candidate only when it still belongs to the
    /// active login attempt. The read-only `/auth/me` probe has already
    /// succeeded before this method is reached, so installation and the
    /// authenticated transition happen under one lock.
    pub(crate) async fn commit_verified_login(
        &self,
        expected_generation: u64,
        cookie_header: Option<String>,
        authorization_header: Option<String>,
        refresh_token: Option<String>,
        expires_at: Option<DateTime<Utc>>,
        request_context: RequestContext,
    ) -> bool {
        let mut inner = self.inner.write().await;
        if inner.login_generation != expected_generation || inner.auth != AuthStatus::LoggingIn {
            return false;
        }

        let last_validated_at = Utc::now();
        inner.credentials = Some(PersistedSession {
            cookie_header,
            authorization_header: normalize_authorization_header(authorization_header),
            refresh_token,
            expires_at,
            last_validated_at: Some(last_validated_at),
            request_context,
        });
        inner.auth = AuthStatus::Authenticated;
        inner.last_validated_at = Some(last_validated_at);
        true
    }

    /// 应用 /auth/refresh 返回的新凭据：轮换 access_token 与 refresh_token，并更新过期时间。
    pub(crate) async fn apply_refreshed_credentials(
        &self,
        authorization_header: Option<String>,
        refresh_token: Option<String>,
        expires_at: Option<DateTime<Utc>>,
    ) {
        let mut inner = self.inner.write().await;
        let Some(credentials) = inner.credentials.as_mut() else {
            return;
        };
        credentials.authorization_header = normalize_authorization_header(authorization_header);
        if refresh_token.is_some() {
            credentials.refresh_token = refresh_token;
        }
        if expires_at.is_some() {
            credentials.expires_at = expires_at;
        }
        credentials.last_validated_at = Some(Utc::now());
        inner.last_validated_at = Some(Utc::now());
        inner.auth = AuthStatus::Authenticated;
    }

    /// Clears the re-login backup only after AppServices has won the matching
    /// close-vs-commit atomic transition. Until then, a synchronous
    /// CloseRequested invalidation can still restore the earlier session.
    pub(crate) async fn finalize_verified_login(&self, expected_generation: u64) {
        let mut inner = self.inner.write().await;
        if inner.login_generation == expected_generation && inner.auth == AuthStatus::Authenticated
        {
            inner.login_backup = None;
        }
    }

    pub(crate) async fn restore_credentials(&self, persisted: PersistedSession) {
        let mut inner = self.inner.write().await;
        inner.login_generation = inner.login_generation.wrapping_add(1);
        inner.last_validated_at = persisted.last_validated_at;
        inner.credentials = Some(persisted);
        inner.login_backup = None;
        inner.auth = AuthStatus::Validating;
    }

    pub(crate) async fn persisted_credentials(&self) -> Option<PersistedSession> {
        self.inner.read().await.credentials.clone()
    }

    pub(crate) async fn request_credentials(&self) -> Result<RequestCredentials, ApiError> {
        let credentials = {
            let inner = self.inner.read().await;
            let Some(credentials) = inner.credentials.as_ref() else {
                return Err(ApiError::MissingCredentials);
            };
            credentials.clone()
        };
        if credentials
            .expires_at
            .is_some_and(|expires_at| expires_at <= Utc::now())
        {
            self.mark_expired().await;
            return Err(ApiError::Unauthorized);
        }
        if credentials.cookie_header.is_none() && credentials.authorization_header.is_none() {
            return Err(ApiError::MissingCredentials);
        }
        Ok(RequestCredentials {
            cookie_header: credentials.cookie_header,
            authorization_header: credentials.authorization_header,
            refresh_token: credentials.refresh_token,
            request_context: credentials.request_context,
        })
    }

    pub(crate) async fn mark_validated(&self) {
        let mut inner = self.inner.write().await;
        // A manual login-window close can clear the newly captured session
        // while its in-flight validation request is still returning. Never let
        // that stale completion turn an empty session back into authenticated.
        if inner.credentials.is_none() {
            return;
        }
        inner.auth = AuthStatus::Authenticated;
        inner.last_validated_at = Some(Utc::now());
        let last_validated_at = inner.last_validated_at;
        if let Some(credentials) = inner.credentials.as_mut() {
            credentials.last_validated_at = last_validated_at;
        }
        inner.login_backup = None;
    }

    pub(crate) async fn mark_expired(&self) {
        let mut inner = self.inner.write().await;
        inner.login_generation = inner.login_generation.wrapping_add(1);
        inner.auth = AuthStatus::Expired;
        inner.credentials = None;
        inner.login_backup = None;
        inner.last_validated_at = None;
    }

    /// Cancels only the pending WebView2 login flow. If the user was already
    /// authenticated, restore that prior session; otherwise discard the
    /// unvalidated handoff and return to the login guide.
    #[cfg(test)]
    pub(crate) async fn cancel_login(&self) -> bool {
        let mut inner = self.inner.write().await;
        cancel_login_inner(&mut inner)
    }

    pub(crate) async fn cancel_login_if_generation(
        &self,
        expected_generation: u64,
    ) -> Option<bool> {
        let mut inner = self.inner.write().await;
        if inner.login_generation != expected_generation {
            return None;
        }
        Some(cancel_login_inner(&mut inner))
    }
}

fn cancel_login_inner(inner: &mut SessionInner) -> bool {
    inner.login_generation = inner.login_generation.wrapping_add(1);
    if let Some(previous) = inner.login_backup.take() {
        inner.credentials = Some(previous);
        inner.auth = AuthStatus::Authenticated;
        true
    } else {
        inner.credentials = None;
        inner.auth = AuthStatus::LoggedOut;
        inner.last_validated_at = None;
        false
    }
}

fn normalize_authorization_header(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let value = value.trim();
        if value.is_empty() || value.chars().any(char::is_control) {
            return None;
        }
        // Compatibility with sessions written by builds that stored only the
        // Bearer token under the former `bearer_token` field.
        Some(if value.chars().any(char::is_whitespace) {
            value.to_owned()
        } else {
            format!("Bearer {value}")
        })
    })
}

impl Session {
    /// Applies only first-party `Set-Cookie` values from a successful request.
    /// Strings remain inside this private session and are never logged.
    pub(crate) async fn apply_set_cookie_headers(&self, headers: &[String]) {
        if headers.is_empty() {
            return;
        }

        let mut inner = self.inner.write().await;
        let Some(credentials) = inner.credentials.as_mut() else {
            return;
        };

        let mut cookies = split_cookie_header(credentials.cookie_header.as_deref());
        let mut updated_expiry = None;
        for header in headers {
            if let Some((name, value, expiry)) = parse_set_cookie(header) {
                if value.is_empty() {
                    cookies.remove(&name);
                } else {
                    cookies.insert(name, value);
                }
                if let Some(expiry) = expiry {
                    updated_expiry = Some(
                        updated_expiry
                            .map_or(expiry, |existing: DateTime<Utc>| existing.max(expiry)),
                    );
                }
            }
        }
        credentials.cookie_header = (!cookies.is_empty()).then(|| {
            cookies
                .into_iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect::<Vec<_>>()
                .join("; ")
        });
        // A server renewal replaces a previous server-declared lifetime. This
        // never invents a client-side extension.
        if updated_expiry.is_some() {
            credentials.expires_at = updated_expiry;
        }
    }
}

fn split_cookie_header(header: Option<&str>) -> BTreeMap<String, String> {
    header
        .unwrap_or_default()
        .split(';')
        .filter_map(|pair| pair.trim().split_once('='))
        .filter_map(|(name, value)| {
            (!name.trim().is_empty()).then(|| (name.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn parse_set_cookie(header: &str) -> Option<(String, String, Option<DateTime<Utc>>)> {
    let mut segments = header.split(';');
    let (name, value) = segments.next()?.trim().split_once('=')?;
    if name.trim().is_empty() {
        return None;
    }

    let mut expiry = None;
    for attribute in segments {
        let Some((key, value)) = attribute.trim().split_once('=') else {
            continue;
        };
        if key.eq_ignore_ascii_case("max-age") {
            if let Ok(seconds) = value.trim().parse::<i64>() {
                expiry = Some(if seconds <= 0 {
                    Utc::now()
                } else {
                    DateTime::<Utc>::from(SystemTime::now() + Duration::from_secs(seconds as u64))
                });
            }
        } else if key.eq_ignore_ascii_case("expires") {
            if let Ok(time) = httpdate::parse_http_date(value.trim()) {
                expiry = Some(DateTime::<Utc>::from(time));
            }
        }
    }

    Some((name.trim().to_string(), value.trim().to_string(), expiry))
}

#[cfg(test)]
mod tests {
    use super::{RequestContext, Session, parse_set_cookie};
    use crate::model::AuthStatus;

    #[tokio::test]
    async fn session_status_never_contains_credentials() {
        let session = Session::new();
        session
            .install_credentials(
                Some("session=test-cookie".to_string()),
                Some("test-token".to_string()),
                None,
                None,
                RequestContext::default(),
            )
            .await;

        let status = session.status().await;
        assert_eq!(status.auth, AuthStatus::Validating);
        let debug = format!("{status:?}");
        assert!(!debug.contains("test-cookie"));
        assert!(!debug.contains("test-token"));
    }

    #[tokio::test]
    async fn cancelling_initial_login_discards_unvalidated_credentials() {
        let session = Session::new();
        session.begin_login().await;
        session
            .install_credentials(
                Some("session=new-cookie".to_string()),
                None,
                None,
                None,
                RequestContext::default(),
            )
            .await;

        assert!(!session.cancel_login().await);
        assert_eq!(session.status().await.auth, AuthStatus::LoggedOut);
        assert!(session.persisted_credentials().await.is_none());
    }

    #[tokio::test]
    async fn stale_validation_cannot_resurrect_a_cancelled_login() {
        let session = Session::new();
        session.begin_login().await;
        session
            .install_credentials(
                Some("session=new-cookie".to_string()),
                None,
                None,
                None,
                RequestContext::default(),
            )
            .await;
        assert!(!session.cancel_login().await);

        // Simulates a `/auth/me` response completing after the user closed
        // the WebView2 login window.
        session.mark_validated().await;
        assert_eq!(session.status().await.auth, AuthStatus::LoggedOut);
        assert!(session.persisted_credentials().await.is_none());
    }

    #[tokio::test]
    async fn cancelled_login_generation_rejects_a_late_cookie_commit() {
        let session = Session::new();
        let generation = session.begin_login().await;
        assert!(!session.cancel_login().await);

        // This is the exact close-vs-poll race: `/auth/me` succeeded, but the
        // window closed before its candidate cookie could be committed.
        assert!(
            !session
                .commit_verified_login(
                    generation,
                    Some("session=late-cookie".to_string()),
                    None,
                    None,
                    None,
                    RequestContext::default(),
                )
                .await
        );
        assert_eq!(session.status().await.auth, AuthStatus::LoggedOut);
        assert!(session.persisted_credentials().await.is_none());
    }

    #[tokio::test]
    async fn verified_relogin_can_still_roll_back_until_commit_finishes() {
        let session = Session::new();
        session
            .install_credentials(
                Some("session=old-cookie".to_string()),
                None,
                None,
                None,
                RequestContext::default(),
            )
            .await;
        session.mark_validated().await;
        let generation = session.begin_login().await;

        assert!(
            session
                .commit_verified_login(
                    generation,
                    Some("session=new-cookie".to_string()),
                    None,
                    None,
                    None,
                    RequestContext::default(),
                )
                .await
        );
        assert!(session.cancel_login_if_generation(generation).await == Some(true));
        let restored = session.persisted_credentials().await.expect("old session");
        assert_eq!(
            restored.cookie_header.as_deref(),
            Some("session=old-cookie")
        );
    }

    #[tokio::test]
    async fn cancelling_relogin_restores_previous_session() {
        let session = Session::new();
        session
            .install_credentials(
                Some("session=old-cookie".to_string()),
                None,
                None,
                None,
                RequestContext::default(),
            )
            .await;

        session.mark_validated().await;
        session.begin_login().await;
        session
            .install_credentials(
                Some("session=new-cookie".to_string()),
                None,
                None,
                None,
                RequestContext::default(),
            )
            .await;

        assert!(session.cancel_login().await);
        assert_eq!(session.status().await.auth, AuthStatus::Authenticated);
        let restored = session.persisted_credentials().await.expect("old session");
        assert_eq!(
            restored.cookie_header.as_deref(),
            Some("session=old-cookie")
        );
    }

    #[test]
    fn set_cookie_parser_reads_expiry_without_exposing_value() {
        let parsed = parse_set_cookie("session=opaque; Max-Age=60; HttpOnly").expect("cookie");
        assert_eq!(parsed.0, "session");
        assert!(parsed.2.is_some());
    }
}
