use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{
    api::error::PublicError,
    model::{DashboardSnapshot, UsageSnapshot},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthStatus {
    #[default]
    LoggedOut,
    LoggingIn,
    Validating,
    Authenticated,
    Expired,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RefreshStatus {
    #[default]
    Idle,
    Refreshing,
    Success,
    Stale,
    Failed,
}

/// Safe in-memory state for IPC and Tauri events. It intentionally owns no
/// credentials and no raw API response bodies.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    pub auth: AuthStatus,
    pub refresh: RefreshStatus,
    pub dashboard: Option<DashboardSnapshot>,
    pub usage: Option<UsageSnapshot>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub last_error: Option<PublicError>,
}

#[derive(Clone)]
pub struct AppState {
    inner: Arc<RwLock<AppSnapshot>>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new(AppSnapshot {
                auth: AuthStatus::Validating,
                ..AppSnapshot::default()
            })),
        }
    }
}

impl AppState {
    pub fn snapshot(&self) -> AppSnapshot {
        self.read().clone()
    }

    pub fn set_auth_status(&self, status: AuthStatus) {
        self.write().auth = status;
    }

    pub fn mark_refreshing(&self) {
        let mut snapshot = self.write();
        snapshot.refresh = RefreshStatus::Refreshing;
        snapshot.last_error = None;
    }

    pub fn replace_success(
        &self,
        dashboard: DashboardSnapshot,
        usage: UsageSnapshot,
        completed_at: DateTime<Utc>,
    ) {
        let mut snapshot = self.write();
        snapshot.dashboard = Some(dashboard);
        snapshot.usage = Some(usage);
        snapshot.refresh = RefreshStatus::Success;
        snapshot.last_success_at = Some(completed_at);
        snapshot.last_error = None;
    }

    /// Preserve the last successful data after ordinary request failures.
    pub fn mark_request_failure(&self, error: PublicError) {
        let mut snapshot = self.write();
        snapshot.refresh = if snapshot.dashboard.is_some() || snapshot.usage.is_some() {
            RefreshStatus::Stale
        } else {
            RefreshStatus::Failed
        };
        snapshot.last_error = Some(error);
    }

    /// A restored encrypted session remains usable after an ordinary startup
    /// request failure. Never overwrite a concurrent login or explicit expiry.
    pub fn retain_restored_session_after_failure(&self, error: PublicError) {
        let mut snapshot = self.write();
        if snapshot.auth != AuthStatus::Validating {
            return;
        }
        snapshot.auth = AuthStatus::Authenticated;
        snapshot.refresh = if snapshot.dashboard.is_some() || snapshot.usage.is_some() {
            RefreshStatus::Stale
        } else {
            RefreshStatus::Failed
        };
        snapshot.last_error = Some(error);
    }

    /// Only an explicit authentication failure clears business snapshots.
    pub fn mark_auth_expired(&self, error: PublicError) {
        let mut snapshot = self.write();
        snapshot.auth = AuthStatus::Expired;
        snapshot.refresh = RefreshStatus::Idle;
        snapshot.dashboard = None;
        snapshot.usage = None;
        snapshot.last_success_at = None;
        snapshot.last_error = Some(error);
    }

    pub fn mark_login_failure(&self, error: PublicError) {
        let mut snapshot = self.write();
        snapshot.auth = AuthStatus::LoggedOut;
        snapshot.refresh = RefreshStatus::Idle;
        snapshot.last_error = Some(error);
    }

    /// Closing the application-owned login window is a user cancellation, not
    /// an authentication failure. It must immediately clear the UI's pending
    /// login state without showing an error or discarding an earlier valid
    /// snapshot/session.
    pub fn cancel_login(&self, restored_previous_session: bool) {
        let mut snapshot = self.write();
        snapshot.auth = if restored_previous_session {
            AuthStatus::Authenticated
        } else {
            AuthStatus::LoggedOut
        };
        if !restored_previous_session {
            snapshot.refresh = RefreshStatus::Idle;
            snapshot.last_error = None;
        }
    }

    fn read(&self) -> RwLockReadGuard<'_, AppSnapshot> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn write(&self) -> RwLockWriteGuard<'_, AppSnapshot> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{AppState, AuthStatus, RefreshStatus};
    use crate::{
        api::error::{PublicError, PublicErrorCode},
        model::{
            ApiKeySummary, DashboardSnapshot, PerformanceSummary, RequestSummary, SpendSummary,
            TokenSummary, UsageRange, UsageSnapshot, UsageSummary,
        },
    };

    fn dashboard() -> DashboardSnapshot {
        DashboardSnapshot {
            balance: None,
            api_keys: ApiKeySummary::default(),
            today_requests: RequestSummary::default(),
            today_spend: SpendSummary::default(),
            today_tokens: TokenSummary::default(),
            cumulative_tokens: TokenSummary::default(),
            performance: PerformanceSummary::default(),
            average_response: None,
            fetched_at: Utc::now(),
        }
    }

    fn usage() -> UsageSnapshot {
        UsageSnapshot {
            range: UsageRange::Last24Hours,
            summary: UsageSummary::default(),
            token_trend: Vec::new(),
            models: Vec::new(),
            groups: Vec::new(),
            endpoints: Vec::new(),
            model_pie: Vec::new(),
            fetched_at: Utc::now(),
        }
    }

    #[test]
    fn network_failure_keeps_last_successful_snapshot() {
        let state = AppState::default();
        state.replace_success(dashboard(), usage(), Utc::now());
        state.mark_request_failure(PublicError::new(PublicErrorCode::Network, "网络失败"));

        let snapshot = state.snapshot();
        assert_eq!(snapshot.refresh, RefreshStatus::Stale);
        assert!(snapshot.dashboard.is_some());
        assert!(snapshot.usage.is_some());
        assert!(snapshot.last_success_at.is_some());
    }

    #[test]
    fn explicit_auth_expiry_is_the_only_failure_that_clears_data() {
        let state = AppState::default();
        state.replace_success(dashboard(), usage(), Utc::now());
        state.set_auth_status(AuthStatus::Authenticated);
        state.mark_auth_expired(PublicError::new(
            PublicErrorCode::AuthenticationRequired,
            "登录状态已失效",
        ));

        let snapshot = state.snapshot();
        assert_eq!(snapshot.auth, AuthStatus::Expired);
        assert!(snapshot.dashboard.is_none());
        assert!(snapshot.usage.is_none());
    }

    #[test]
    fn cancelling_login_clears_pending_state_without_an_error() {
        let state = AppState::default();
        state.set_auth_status(AuthStatus::LoggingIn);
        state.cancel_login(false);

        let snapshot = state.snapshot();
        assert_eq!(snapshot.auth, AuthStatus::LoggedOut);
        assert_eq!(snapshot.refresh, RefreshStatus::Idle);
        assert!(snapshot.last_error.is_none());
    }

    #[test]
    fn restored_session_survives_an_ordinary_startup_failure() {
        let state = AppState::default();
        state.retain_restored_session_after_failure(PublicError::new(
            PublicErrorCode::Network,
            "网络失败",
        ));

        let snapshot = state.snapshot();
        assert_eq!(snapshot.auth, AuthStatus::Authenticated);
        assert_eq!(snapshot.refresh, RefreshStatus::Failed);
        assert_eq!(
            snapshot.last_error.as_ref().map(|error| error.code),
            Some(PublicErrorCode::Network)
        );
    }

    #[test]
    fn retained_restore_never_overwrites_a_newer_auth_transition() {
        let state = AppState::default();
        state.mark_auth_expired(PublicError::new(
            PublicErrorCode::AuthenticationRequired,
            "登录状态已失效",
        ));
        state.retain_restored_session_after_failure(PublicError::new(
            PublicErrorCode::Network,
            "网络失败",
        ));
        assert_eq!(state.snapshot().auth, AuthStatus::Expired);

        state.set_auth_status(AuthStatus::LoggingIn);
        state.retain_restored_session_after_failure(PublicError::new(
            PublicErrorCode::Timeout,
            "请求超时",
        ));
        assert_eq!(state.snapshot().auth, AuthStatus::LoggingIn);
    }
}
