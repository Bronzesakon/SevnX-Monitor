use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering},
    },
};

const NO_LOGIN_ATTEMPT: u64 = 0;
const LOGIN_COMMITTING_BIT: u64 = 1 << 63;
const LOGIN_CANCELLED_BIT: u64 = 1 << 62;
const LOGIN_ATTEMPT_MASK: u64 = !(LOGIN_COMMITTING_BIT | LOGIN_CANCELLED_BIT);
/// access_token 剩余有效期低于该值时触发 /auth/refresh 预热。
const RENEWAL_MARGIN_SECS: i64 = 30 * 60;
/// 两次 refresh 尝试的最小间隔，避免在服务器不续期时反复空转。
const RENEWAL_DEBOUNCE_SECS: i64 = 60;

use tokio::sync::{RwLock, mpsc};

use crate::{
    api::{
        client::{SevnxApiClient, VerifiedUsageQuery},
        error::{ApiError, PublicError, PublicErrorCode},
    },
    auth::{CredentialStore, Session},
    model::{AppSettings, AppSnapshot, AppState, AuthStatus, UsageRange},
    services::{
        logging::SafeLog,
        codex_inject,
        overlay_server::{self, OverlayAction, OverlayServerInfo},
        protocol,
        refresh_scheduler::RefreshCoordinator,
    },
    storage::{
        AppPaths, PersistedSettings, SettingsStore, StorageError, StoredAlertState,
        StoredBarPosition,
    },
};

/// Application-owned services. Fields that can carry credentials are private;
/// Tauri commands only return the whitelist-shaped `AppSnapshot` and settings.
pub struct AppServices {
    state: AppState,
    session: Session,
    credential_store: CredentialStore,
    paths: AppPaths,
    settings_store: SettingsStore,
    persisted_settings: RwLock<PersistedSettings>,
    refresh: RefreshCoordinator<SevnxApiClient>,
    usage_range: RwLock<UsageRange>,
    client: Arc<SevnxApiClient>,
    login_capture_in_flight: AtomicBool,
    login_attempt_state: AtomicU64,
    login_profile: Mutex<Option<LoginProfile>>,
    last_refresh_attempt: AtomicI64,
    safe_log: SafeLog,
}

/// The short-lived WebView2 profile is bound to one login attempt. Keeping the
/// path private prevents it from leaking through IPC and lets a new attempt use
/// a fresh directory even while WebView2 is still releasing the old one.
struct LoginProfile {
    attempt: u64,
    path: PathBuf,
}

impl AppServices {
    pub fn new() -> Result<Self, StorageError> {
        let paths = AppPaths::discover()?;
        let safe_log = SafeLog::new(paths.logs_dir());
        // Self-register the `sevnx://` protocol so Codex-overlay reconnect works
        // on every machine. Idempotent and keeps the command path current.
        if let Ok(exe) = std::env::current_exe() {
            match protocol::register_sevnx_protocol(&exe) {
                Ok(()) => safe_log.write("protocol_register_ok", "result=registered"),
                Err(error) => safe_log.write_dynamic(
                    "protocol_register_failed",
                    format!("error={error}"),
                ),
            }
        }
        let settings_store = SettingsStore::new(paths.settings_file());
        let persisted_settings = match settings_store.load() {
            Ok(settings) => settings,
            Err(error) => {
                safe_log.write("settings_load_failed", storage_error_label(&error));
                return Err(error);
            }
        };
        let state = AppState::default();
        let session = Session::new();
        let client = match SevnxApiClient::new(session.clone()) {
            Ok(client) => Arc::new(client),
            Err(_) => {
                safe_log.write("http_client_initialization_failed", "error=unavailable");
                return Err(StorageError::Unavailable);
            }
        };
        client.set_default_api_origin(&persisted_settings.settings.access_url);
        let refresh = RefreshCoordinator::new(client.clone(), state.clone());
        safe_log.write("app_services_initialized", "result=success");
        Ok(Self {
            state,
            session,
            credential_store: CredentialStore::new(paths.session_file()),
            paths,
            settings_store,
            persisted_settings: RwLock::new(persisted_settings),
            refresh,
            usage_range: RwLock::new(UsageRange::Last24Hours),
            client,
            login_capture_in_flight: AtomicBool::new(false),
            login_attempt_state: AtomicU64::new(NO_LOGIN_ATTEMPT),
            login_profile: Mutex::new(None),
            last_refresh_attempt: AtomicI64::new(0),
            safe_log,
        })
    }

    pub fn snapshot(&self) -> AppSnapshot {
        self.state.snapshot()
    }

    /// 是否已持有 refresh_token（脱敏布尔），供诊断摘要展示自动续期状态。
    pub fn has_refresh_token(&self) -> bool {
        self.session.has_refresh_token()
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    /// Starts the loopback overlay HTTP server and persists only the port to
    /// disk (the bearer token stays in memory, see design §12/§17-M1).
    pub(crate) async fn start_overlay(
        &self,
        action_tx: mpsc::Sender<OverlayAction>,
    ) -> Result<OverlayServerInfo, std::io::Error> {
        let info = match overlay_server::spawn(self.state.clone(), self.safe_log.clone(), action_tx)
            .await
        {
            Ok(info) => info,
            Err(error) => {
                self.safe_log
                    .write_error("overlay_server_failed", "reason=bind_failed");
                return Err(error);
            }
        };
        self.write_overlay_meta(info.port);
        self.safe_log
            .write_dynamic("overlay_ready", format!("port={}", info.port));
        Ok(info)
    }

    /// Exposes the shared audit sink so long-running tasks (e.g. the injection
    /// watchdog) can log without owning the whole `AppServices`.
    pub(crate) fn logger(&self) -> SafeLog {
        self.safe_log.clone()
    }

    /// Injects the overlay UI into the Codex page listening on `debug_port`.
    /// Data is delivered afterwards by the watchdog's CDP push, so nothing
    /// secret ever enters the injected script.
    pub(crate) async fn inject_codex_overlay(&self, debug_port: u16) -> Result<(), String> {
        self.safe_log.write_dynamic(
            "codex_inject_started",
            format!("debug_port={debug_port}"),
        );
        codex_inject::inject_overlay(debug_port).await.map_err(|error| {
            self.safe_log
                .write_dynamic("codex_inject_failed", format!("error={error}"));
            error
        })?;
        self.safe_log
            .write("codex_inject_ok", "result=injected");
        Ok(())
    }

    /// Whitelisted snapshot JSON for the CDP data push (bar + detail window).
    pub(crate) fn overlay_push_json(&self) -> String {
        overlay_server::detail_payload_json(&self.state.snapshot())
    }

    /// Wakes the injection watchdog as soon as a public overlay field changes.
    pub(crate) async fn wait_for_overlay_update(&self) {
        self.state.notify().notified().await;
    }

    fn write_overlay_meta(&self, port: u16) {
        let path = self.paths.overlay_meta_file();
        let json = serde_json::json!({ "overlayPort": port });
        match serde_json::to_vec_pretty(&json) {
            Ok(bytes) => {
                if std::fs::write(&path, bytes).is_err() {
                    self.safe_log.write("overlay_meta_write_failed", "reason=io");
                }
            }
            Err(_) => self
                .safe_log
                .write("overlay_meta_write_failed", "reason=serialize"),
        }
    }

    pub fn session(&self) -> Session {
        self.session.clone()
    }

    pub fn client(&self) -> Arc<SevnxApiClient> {
        self.client.clone()
    }

    pub async fn settings(&self) -> AppSettings {
        self.persisted_settings.read().await.settings.clone()
    }

    pub async fn update_settings(&self, next: AppSettings) -> Result<AppSettings, PublicError> {
        if let Err(message) = next.validate() {
            self.safe_log
                .write("settings_validation_failed", "error=invalid_response");
            return Err(PublicError::new(PublicErrorCode::InvalidResponse, message));
        }
        let mut persisted = self.persisted_settings.write().await;
        persisted.settings = next.clone();
        self.client.set_default_api_origin(&next.access_url);
        let save_result = self.settings_store.save(&persisted);
        drop(persisted);
        if let Err(error) = save_result {
            self.safe_log
                .write("settings_save_failed", storage_error_label(&error));
            return Err(storage_to_public(error));
        }
        Ok(next)
    }

    pub async fn set_usage_range(&self, range: UsageRange) {
        *self.usage_range.write().await = range;
    }

    pub async fn usage_range(&self) -> UsageRange {
        *self.usage_range.read().await
    }

    pub async fn refresh_current(&self) -> Result<AppSnapshot, PublicError> {
        self.renew_session_if_near_expiry().await;
        let range = self.usage_range().await;
        let query = match VerifiedUsageQuery::for_range(range) {
            Ok(query) => query,
            Err(error) => {
                let public = error.to_public();
                self.log_public_error("refresh_query_failed", &public);
                return Err(public);
            }
        };
        let result = self.refresh.refresh_all(range, query).await;
        match result {
            Ok(snapshot) => {
                // Session renewal is only persisted after a successful server
                // response, and the persistence error is public but never
                // contains credential material.
                if let Err(error) = self.persist_current_session().await {
                    self.log_public_error("refresh_session_persist_failed", &error);
                    return Err(error);
                }
                Ok(snapshot)
            }
            Err(error) => {
                self.log_public_error("refresh_failed", &error);
                if self.snapshot().auth == AuthStatus::Expired {
                    self.delete_credentials("reason=refresh_auth_expired");
                }
                Err(error)
            }
        }
    }

    /// access_token 临近过期时，用 refresh_token 调 /auth/refresh 换新；
    /// 成功即持久化新凭据。带去抖，避免在服务器不续期时反复空转。
    async fn renew_session_if_near_expiry(&self) {
        let Some(expires_at) = self.session.status().await.expires_at else {
            return;
        };
        let remaining = (expires_at - chrono::Utc::now()).num_seconds();
        // 未接近到期时直接返回，不写日志，避免每 30s 刷屏。
        if remaining > RENEWAL_MARGIN_SECS {
            return;
        }
        let now = chrono::Utc::now().timestamp();
        let last = self.last_refresh_attempt.load(Ordering::Relaxed);
        if now - last < RENEWAL_DEBOUNCE_SECS {
            return;
        }
        self.last_refresh_attempt.store(now, Ordering::Relaxed);
        // 到期前触发续期是正常但关键的事件，用 CRITICAL 记一次，时间用本地时区。
        let local_expires = expires_at.with_timezone(&chrono::Local);
        self.safe_log.write_critical_dynamic(
            "session_renewal_triggered",
            format!(
                "reason=near_expiry expires={}",
                local_expires.format("%Y-%m-%d %H:%M:%S")
            ),
        );
        match self.client.refresh_session().await {
            Ok(()) => {
                self.persist_renewed_session("session_refresh_succeeded")
                    .await;
            }
            Err(error) => {
                let public = error.to_public();
                self.apply_session_refresh_failure(
                    &error,
                    public,
                    "reason=session_refresh_auth_invalid",
                )
                .await;
                self.safe_log
                    .write("session_refresh_failed", refresh_failure_label(&error));
            }
        }
    }

    /// Every restored login gets one unconditional refresh-token exchange
    /// before the first dashboard request. The normal 30-minute pre-expiry
    /// renewal remains in `renew_session_if_near_expiry` for the rest of the
    /// process lifetime.
    async fn renew_session_on_restore(&self) -> Result<(), ApiError> {
        self.last_refresh_attempt
            .store(chrono::Utc::now().timestamp(), Ordering::Relaxed);
        self.safe_log
            .write_critical("session_startup_refresh_triggered", "source=restore");
        match self.client.refresh_session().await {
            Ok(()) => {
                self.persist_renewed_session("session_startup_refresh_succeeded")
                    .await;
                Ok(())
            }
            Err(error) => {
                self.safe_log.write(
                    "session_startup_refresh_failed",
                    refresh_failure_label(&error),
                );
                Err(error)
            }
        }
    }

    async fn persist_renewed_session(&self, success_event: &'static str) {
        if let Err(error) = self.persist_current_session().await {
            self.log_public_error("session_refresh_credential_save_failed", &error);
            return;
        }
        let next = self
            .session
            .status()
            .await
            .expires_at
            .map(|value| {
                value
                    .with_timezone(&chrono::Local)
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_default();
        self.safe_log
            .write_dynamic(success_event, format!("result=success next_expires={next}"));
    }

    /// Keeps the public state in lockstep with a refresh-token failure. A
    /// rejected token, or a session already cleared because its access token
    /// expired, is terminal; transient failures retain the current session.
    async fn apply_session_refresh_failure(
        &self,
        error: &ApiError,
        public: PublicError,
        delete_reason: &'static str,
    ) -> bool {
        if error.is_auth_invalid() || self.session.status().await.auth == AuthStatus::Expired {
            self.delete_credentials(delete_reason);
            self.state.mark_auth_expired(public);
            true
        } else {
            self.state.mark_request_failure(public);
            false
        }
    }

    pub async fn begin_login(&self) -> u64 {
        let attempt = self.session.begin_login().await;
        self.login_attempt_state.store(attempt, Ordering::Release);
        self.state.set_auth_status(AuthStatus::LoggingIn);
        self.safe_log.write("login_started", "source=webview");
        attempt
    }

    /// Called when the user closes the isolated WebView2 login window. This is
    /// deliberately separate from an auth failure: closing the window must
    /// never leave the main UI in `loggingIn`/`validating` indefinitely.
    pub async fn cancel_login_window(&self, attempt: u64) -> bool {
        let Some(restored_previous_session) =
            self.session.cancel_login_if_generation(attempt).await
        else {
            return false;
        };
        self.state.cancel_login(restored_previous_session);
        self.finish_login_capture();
        self.safe_log.write(
            "login_window_closed",
            if restored_previous_session {
                "result=restored_previous_session"
            } else {
                "result=cancelled"
            },
        );
        true
    }

    pub fn login_is_pending(&self) -> bool {
        matches!(
            self.snapshot().auth,
            AuthStatus::LoggingIn | AuthStatus::Validating
        )
    }

    /// Checks the login attempt captured before the `/auth/me` probe. It is
    /// intentionally separate from the UI snapshot: a close event invalidates
    /// the session generation before asynchronous event delivery reaches Vue.
    pub async fn is_current_login_attempt(&self, attempt: u64) -> bool {
        self.login_attempt_state.load(Ordering::Acquire) == attempt
            && self.session.login_generation().await == attempt
            && self.login_is_pending()
    }

    pub fn begin_login_capture(&self) -> bool {
        let started = self
            .login_capture_in_flight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok();
        if !started {
            self.safe_log
                .write("login_capture_skipped", "reason=already_in_flight");
        }
        started
    }

    pub fn finish_login_capture(&self) {
        self.login_capture_in_flight.store(false, Ordering::Release);
    }

    pub async fn fail_login_capture(&self, attempt: u64, error: PublicError) -> bool {
        self.log_public_error("login_credential_capture_failed", &error);
        if !self.cancel_login_attempt(attempt) {
            return false;
        }
        let Some(restored_previous_session) =
            self.session.cancel_login_if_generation(attempt).await
        else {
            return false;
        };
        self.state.cancel_login(restored_previous_session);
        self.state.mark_request_failure(error);
        self.finish_login_capture();
        true
    }

    /// Records only the safe error category when the isolated WebView2 window
    /// itself cannot be opened. The command owns the window operation, while
    /// AppServices owns the application's safe audit sink.
    pub async fn record_login_window_failure(&self, attempt: u64, error: &PublicError) {
        self.log_public_error("login_window_open_failed", error);
        if !self.cancel_login_attempt(attempt) {
            return;
        }
        let Some(restored_previous_session) =
            self.session.cancel_login_if_generation(attempt).await
        else {
            return;
        };
        self.state.cancel_login(restored_previous_session);
        self.finish_login_capture();
        // A failed window creation is actionable, but it is not an
        // authentication failure and must not leave the UI in `loggingIn`.
        self.state.mark_request_failure(error.clone());
    }

    /// Records a fixed window-restore stage without exposing platform error
    /// text, which may contain environment-specific details.
    pub(crate) fn record_main_window_restore_failure(&self, stage: &'static str) {
        self.safe_log.write("main_window_restore_failed", stage);
    }

    /// Commits credentials only after a transient CookieManager and/or
    /// whitelisted-storage candidate has succeeded against the Dashboard
    /// stats endpoint.
    /// Failed probes from an in-progress login page never enter this session
    /// or alter the previous login state.
    pub(crate) async fn complete_login_from_verified_credentials(
        &self,
        attempt: u64,
        cookie_header: Option<String>,
        authorization_header: Option<String>,
        refresh_token: Option<String>,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
        request_context: crate::auth::RequestContext,
    ) -> Result<Option<AppSnapshot>, PublicError> {
        if cookie_header.is_none() && authorization_header.is_none() {
            return Err(PublicError::new(
                PublicErrorCode::AuthenticationRequired,
                "未检测到登录凭证",
            ));
        }
        if !self.claim_login_attempt_for_commit(attempt) {
            return Ok(None);
        }
        if !self
            .session
            .commit_verified_login(
                attempt,
                cookie_header,
                authorization_header,
                refresh_token,
                expires_at,
                request_context,
            )
            .await
        {
            self.abandon_login_attempt_commit(attempt);
            return Ok(None);
        }
        if !self.finish_login_attempt_commit(attempt) {
            if let Some(restored_previous_session) =
                self.session.cancel_login_if_generation(attempt).await
            {
                self.state.cancel_login(restored_previous_session);
            }
            return Ok(None);
        }
        self.session.finalize_verified_login(attempt).await;
        self.safe_log
            .write("login_credentials_captured", "result=received");
        self.state.set_auth_status(AuthStatus::Authenticated);
        self.finalize_validated_login().await.map(Some)
    }

    /// Login polling records only stable categories, never browser values,
    /// storage-key names, request headers, or response text. These events are
    /// intentionally narrow so the next real login can distinguish capture
    /// failures from a rejected `/auth/me` candidate.
    pub(crate) fn record_login_cookie_capture_failure(&self) {
        self.safe_log
            .write("login_cookie_capture_failed", "source=cookie_manager");
    }

    pub(crate) fn record_login_storage_capture_failure(&self) {
        self.safe_log
            .write("login_storage_capture_failed", "source=execute_script");
    }

    pub(crate) fn record_login_candidate_missing(&self) {
        self.safe_log
            .write("login_candidate_missing", "source=cookie_and_storage");
    }

    pub(crate) fn record_login_candidate_observed(&self, has_cookie: bool, has_bearer: bool) {
        let source = match (has_cookie, has_bearer) {
            (true, true) => "source=cookie_and_bearer",
            (true, false) => "source=cookie",
            (false, true) => "source=bearer",
            (false, false) => "source=none",
        };
        self.safe_log.write("login_candidate_observed", source);
    }

    pub(crate) fn record_login_candidate_validated(&self, has_cookie: bool, has_bearer: bool) {
        let source = match (has_cookie, has_bearer) {
            (true, true) => "source=cookie_and_bearer",
            (true, false) => "source=cookie",
            (false, true) => "source=bearer",
            (false, false) => "source=none",
        };
        self.safe_log.write("login_candidate_validated", source);
    }

    pub(crate) fn record_login_candidate_validation_failure(&self, error: &ApiError) {
        self.safe_log.write(
            "login_candidate_validation_failed",
            candidate_validation_error_label(error),
        );
    }

    pub(crate) fn record_login_request_credentials_observed(
        &self,
        has_cookie: bool,
        has_authorization: bool,
    ) {
        self.safe_log.write(
            "login_request_credentials_observed",
            request_credential_source_label(has_cookie, has_authorization),
        );
    }

    pub(crate) fn record_login_request_context(
        &self,
        request_kind: &'static str,
        authorization_scheme: &'static str,
        has_origin: bool,
        has_referer: bool,
    ) {
        self.safe_log.write(
            "login_request_context",
            login_request_context_label(
                request_kind,
                authorization_scheme,
                has_origin,
                has_referer,
            ),
        );
    }

    pub(crate) fn record_login_request_path(&self, path: &str) {
        self.safe_log.write_login_api_path(path);
    }

    pub(crate) fn record_login_request_credentials_validated(
        &self,
        has_cookie: bool,
        has_authorization: bool,
    ) {
        self.safe_log.write(
            "login_request_credentials_validated",
            request_credential_source_label(has_cookie, has_authorization),
        );
    }

    pub(crate) fn record_login_request_credentials_validation_failure(&self, error: &ApiError) {
        self.safe_log.write(
            "login_request_credentials_validation_failed",
            candidate_validation_error_label(error),
        );
    }

    pub(crate) fn record_login_dashboard_detected(&self) {
        self.safe_log
            .write("login_dashboard_detected", "route=dashboard");
    }

    /// Called directly from `CloseRequested`, before the async cancellation
    /// task is queued. A late CookieManager poll can only claim an unchanged
    /// active attempt, so this immediately blocks stale credential commits.
    pub fn cancel_login_attempt_from_window(&self) -> Option<u64> {
        loop {
            let current = self.login_attempt_state.load(Ordering::Acquire);
            if current == NO_LOGIN_ATTEMPT || current & LOGIN_CANCELLED_BIT != 0 {
                return None;
            }
            let attempt = current & LOGIN_ATTEMPT_MASK;
            if attempt == NO_LOGIN_ATTEMPT {
                return None;
            }
            let cancelled = attempt | LOGIN_CANCELLED_BIT;
            if self
                .login_attempt_state
                .compare_exchange(current, cancelled, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(attempt);
            }
        }
    }

    fn claim_login_attempt_for_commit(&self, attempt: u64) -> bool {
        self.login_attempt_state
            .compare_exchange(
                attempt,
                attempt | LOGIN_COMMITTING_BIT,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn finish_login_attempt_commit(&self, attempt: u64) -> bool {
        self.login_attempt_state
            .compare_exchange(
                attempt | LOGIN_COMMITTING_BIT,
                NO_LOGIN_ATTEMPT,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn abandon_login_attempt_commit(&self, attempt: u64) {
        let _ = self.login_attempt_state.compare_exchange(
            attempt | LOGIN_COMMITTING_BIT,
            NO_LOGIN_ATTEMPT,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    fn cancel_login_attempt(&self, attempt: u64) -> bool {
        loop {
            let current = self.login_attempt_state.load(Ordering::Acquire);
            if current & LOGIN_ATTEMPT_MASK != attempt || current & LOGIN_CANCELLED_BIT != 0 {
                return false;
            }
            if self
                .login_attempt_state
                .compare_exchange(
                    current,
                    attempt | LOGIN_CANCELLED_BIT,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    pub fn register_login_profile(&self, attempt: u64, path: PathBuf) {
        *self
            .login_profile
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) =
            Some(LoginProfile { attempt, path });
    }

    /// A repeat request can target the already visible WebView2 window. Keep
    /// its profile, but transfer ownership to the newer attempt so an older
    /// polling task cannot clean up or validate the newer flow.
    pub fn reuse_login_profile(&self, attempt: u64) -> Option<PathBuf> {
        let mut profile = self
            .login_profile
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let profile = profile.as_mut()?;
        profile.attempt = attempt;
        Some(profile.path.clone())
    }

    pub fn take_login_profile_for_attempt(&self, attempt: u64) -> Option<PathBuf> {
        let mut profile = self
            .login_profile
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if profile
            .as_ref()
            .is_some_and(|current| current.attempt == attempt)
        {
            return profile.take().map(|current| current.path);
        }
        None
    }

    /// Called synchronously from the login window's close event, before a
    /// fast re-open can create another window with the same Tauri label.
    pub fn take_active_login_profile(&self) -> Option<PathBuf> {
        self.login_profile
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
            .map(|profile| profile.path)
    }

    /// Restores encrypted credentials and uses the first complete refresh as
    /// the startup authentication check. Ordinary request failures retain the
    /// session; only an explicit authentication response expires it.
    pub async fn restore_and_refresh_session(&self) -> AppSnapshot {
        self.safe_log
            .write("session_restore_started", "source=dpapi");
        let persisted = match self.credential_store.load() {
            Ok(Some(session)) => session,
            Ok(None) => {
                self.safe_log
                    .write("session_restore_absent", "result=logged_out");
                self.state.set_auth_status(AuthStatus::LoggedOut);
                return self.snapshot();
            }
            Err(error) => {
                self.safe_log
                    .write("session_restore_failed", storage_error_label(&error));
                self.state.set_auth_status(AuthStatus::LoggedOut);
                self.state.mark_request_failure(PublicError::new(
                    PublicErrorCode::Internal,
                    "无法读取本地登录凭证",
                ));
                return self.snapshot();
            }
        };

        self.session.restore_credentials(persisted).await;
        self.state.set_auth_status(AuthStatus::Validating);
        if let Err(error) = self.renew_session_on_restore().await {
            let public = error.to_public();
            if self
                .apply_session_refresh_failure(
                    &error,
                    public,
                    "reason=restore_refresh_auth_invalid",
                )
                .await
            {
                return self.snapshot();
            }
        }
        if let Err(error) = self.refresh_current().await {
            self.state.retain_restored_session_after_failure(error);
        }
        self.snapshot()
    }

    pub async fn validate_new_login(&self) -> Result<AppSnapshot, PublicError> {
        self.safe_log
            .write("login_validation_started", "source=webview");
        match self.client.validate_session().await {
            Ok(()) => {
                // The login window may have been closed while the read-only
                // validation request was in flight. In that case the session
                // has already been cancelled and this stale response must not
                // resurrect the pending-login UI state.
                if self.session.status().await.auth != AuthStatus::Authenticated {
                    return Err(PublicError::new(
                        PublicErrorCode::AuthenticationRequired,
                        "登录窗口已关闭",
                    ));
                }
                self.finalize_validated_login().await
            }
            Err(error) if error.is_auth_invalid() => {
                let public = error.to_public();
                self.log_public_error("login_validation_auth_invalid", &public);
                self.session.mark_expired().await;
                self.delete_credentials("reason=login_validation_auth_invalid");
                self.state.mark_auth_expired(public.clone());
                Err(public)
            }
            Err(error) => {
                let public = error.to_public();
                self.log_public_error("login_validation_failed", &public);
                self.state.mark_request_failure(public.clone());
                Err(public)
            }
        }
    }

    async fn finalize_validated_login(&self) -> Result<AppSnapshot, PublicError> {
        if let Err(error) = self.persist_current_session().await {
            self.log_public_error("login_session_persist_failed", &error);
            return Err(error);
        }
        self.state.set_auth_status(AuthStatus::Authenticated);
        self.mark_onboarding_complete().await;
        self.safe_log.write("login_validated", "result=success");
        // Successful `/auth/me` validation completes login even if the first
        // full data refresh encounters a normal network error. That error is
        // reflected in the snapshot but must not keep the login window open or
        // discard the new valid session.
        let _ = self.refresh_current().await;
        Ok(self.snapshot())
    }

    pub async fn bar_position(&self) -> Option<StoredBarPosition> {
        self.persisted_settings.read().await.bar_position.clone()
    }

    pub fn initial_bar_state(&self) -> (bool, Option<StoredBarPosition>) {
        self.persisted_settings
            .try_read()
            .map(|settings| (settings.settings.bar_visible, settings.bar_position.clone()))
            .unwrap_or((true, None))
    }

    pub async fn set_bar_position(&self, position: StoredBarPosition) {
        let mut persisted = self.persisted_settings.write().await;
        persisted.bar_position = Some(position);
        let save_result = self.settings_store.save(&persisted);
        drop(persisted);
        if let Err(error) = save_result {
            self.safe_log
                .write("bar_position_save_failed", storage_error_label(&error));
        }
    }

    pub async fn alert_state(&self) -> StoredAlertState {
        self.persisted_settings.read().await.alert_state.clone()
    }

    pub async fn set_alert_state(&self, state: StoredAlertState) {
        let mut persisted = self.persisted_settings.write().await;
        persisted.alert_state = state;
        let save_result = self.settings_store.save(&persisted);
        drop(persisted);
        if let Err(error) = save_result {
            self.safe_log
                .write("alert_state_save_failed", storage_error_label(&error));
        }
    }

    async fn mark_onboarding_complete(&self) {
        let mut persisted = self.persisted_settings.write().await;
        persisted.onboarding_complete = true;
        let save_result = self.settings_store.save(&persisted);
        drop(persisted);
        if let Err(error) = save_result {
            self.safe_log
                .write("onboarding_state_save_failed", storage_error_label(&error));
        }
    }

    /// 调试用：无条件用 refresh_token 触发 `/auth/refresh`，把各环节结果写入日志
    /// 供分析。与自动续期（仅临近过期才刷新）不同，这里忽略剩余时间直接执行。
    pub async fn refresh_session_now(&self) -> Result<(), PublicError> {
        let has_refresh = self.has_refresh_token();
        // 用 event 名区分「是否持有 refresh_token」，避免 detail 里的
        // "has_refresh_token=true/false" 被安全脱敏（正则误伤布尔值）。
        self.safe_log.write(
            if has_refresh {
                "debug_refresh_started_with_token"
            } else {
                "debug_refresh_started_without_token"
            },
            "source=manual_test",
        );
        match self.client.refresh_session().await {
            Ok(()) => {
                if let Err(error) = self.persist_current_session().await {
                    self.log_public_error("debug_refresh_persist_failed", &error);
                    return Err(error);
                }
                self.safe_log
                    .write("debug_refresh_completed", "result=success");
                Ok(())
            }
            Err(error) => {
                let public = error.to_public();
                self.apply_session_refresh_failure(
                    &error,
                    public.clone(),
                    "reason=debug_refresh_auth_invalid",
                )
                .await;
                self.log_public_error("debug_refresh_failed", &public);
                Err(public)
            }
        }
    }

    async fn persist_current_session(&self) -> Result<(), PublicError> {
        let Some(session) = self.session.persisted_credentials().await else {
            return Ok(());
        };
        match self.credential_store.save(&session) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.safe_log
                    .write("credential_persist_failed", storage_error_label(&error));
                Err(storage_to_public(error))
            }
        }
    }

    fn delete_credentials(&self, reason: &'static str) {
        if self.credential_store.delete().is_err() {
            self.safe_log.write("credential_delete_failed", reason);
        }
    }

    fn log_public_error(&self, event: &'static str, error: &PublicError) {
        // PublicError::message is intentionally omitted. Today it is generic,
        // but retaining that invariant here protects logs if a future UI error
        // ever includes server- or user-supplied text.
        self.safe_log
            .write_error(event, public_error_code_label(error.code));
    }
}

fn storage_to_public(_error: StorageError) -> PublicError {
    PublicError::new(PublicErrorCode::Internal, "无法安全保存本地应用数据")
}

fn storage_error_label(error: &StorageError) -> &'static str {
    match error {
        StorageError::Unavailable => "error=storage_unavailable",
        StorageError::Read => "error=storage_read",
        StorageError::Write => "error=storage_write",
        StorageError::Decode => "error=storage_decode",
        StorageError::Protect => "error=storage_protect",
        StorageError::Unprotect => "error=storage_unprotect",
    }
}

fn public_error_code_label(code: PublicErrorCode) -> &'static str {
    match code {
        PublicErrorCode::Network => "error=network",
        PublicErrorCode::Timeout => "error=timeout",
        PublicErrorCode::AuthenticationRequired => "error=authentication_required",
        PublicErrorCode::PermissionDenied => "error=permission_denied",
        PublicErrorCode::RateLimited => "error=rate_limited",
        PublicErrorCode::ServiceUnavailable => "error=service_unavailable",
        PublicErrorCode::InvalidResponse => "error=invalid_response",
        PublicErrorCode::BusinessRejected => "error=business_rejected",
        PublicErrorCode::NotReady => "error=not_ready",
        PublicErrorCode::Internal => "error=internal",
    }
}

fn candidate_validation_error_label(error: &ApiError) -> &'static str {
    match error {
        ApiError::Network => "error=network",
        ApiError::Timeout => "error=timeout",
        ApiError::Unauthorized => "error=unauthorized",
        ApiError::Forbidden => "error=forbidden",
        ApiError::RateLimited => "error=rate_limited",
        ApiError::ServiceUnavailable => "error=service_unavailable",
        ApiError::HttpStatus => "error=http_status",
        ApiError::BusinessRejected { .. } => "error=business_rejected",
        ApiError::InvalidResponse { .. } => "error=invalid_response",
        ApiError::MissingCredentials => "error=missing_credentials",
    }
}

fn refresh_failure_label(error: &ApiError) -> &'static str {
    match error {
        ApiError::MissingCredentials => "reason=refresh_credential_missing",
        ApiError::Unauthorized => "reason=refresh_credential_rejected",
        ApiError::Forbidden => "reason=refresh_credential_forbidden",
        ApiError::Network => "reason=network",
        ApiError::Timeout => "reason=timeout",
        ApiError::RateLimited => "reason=rate_limited",
        ApiError::ServiceUnavailable => "reason=service_unavailable",
        ApiError::HttpStatus => "reason=http_status",
        ApiError::BusinessRejected { .. } => "reason=business_rejected",
        ApiError::InvalidResponse { .. } => "reason=invalid_response",
    }
}

fn request_credential_source_label(has_cookie: bool, has_authorization: bool) -> &'static str {
    match (has_cookie, has_authorization) {
        (true, true) => "source=cookie_and_authorization",
        (true, false) => "source=cookie",
        (false, true) => "source=authorization",
        (false, false) => "source=none",
    }
}

fn login_request_context_label(
    request_kind: &'static str,
    authorization_scheme: &'static str,
    has_origin: bool,
    has_referer: bool,
) -> &'static str {
    match (request_kind, authorization_scheme, has_origin, has_referer) {
        ("auth_me", "bearer", true, true) => {
            "request=auth_me auth_scheme=bearer origin=yes referer=yes"
        }
        ("auth_other", "bearer", true, true) => {
            "request=auth_other auth_scheme=bearer origin=yes referer=yes"
        }
        ("dashboard_stats", "bearer", true, true) => {
            "request=dashboard_stats auth_scheme=bearer origin=yes referer=yes"
        }
        ("dashboard_models", "bearer", true, true) => {
            "request=dashboard_models auth_scheme=bearer origin=yes referer=yes"
        }
        ("dashboard_snapshot", "bearer", true, true) => {
            "request=dashboard_snapshot auth_scheme=bearer origin=yes referer=yes"
        }
        ("usage_stats", "bearer", true, true) => {
            "request=usage_stats auth_scheme=bearer origin=yes referer=yes"
        }
        ("usage_other", "bearer", true, true) => {
            "request=usage_other auth_scheme=bearer origin=yes referer=yes"
        }
        ("user", "bearer", true, true) => "request=user auth_scheme=bearer origin=yes referer=yes",
        ("api_keys", "bearer", true, true) => {
            "request=api_keys auth_scheme=bearer origin=yes referer=yes"
        }
        ("balance", "bearer", true, true) => {
            "request=balance auth_scheme=bearer origin=yes referer=yes"
        }
        (_, "bearer", true, true) => "request=other auth_scheme=bearer origin=yes referer=yes",
        (_, "bearer", true, false) => "request=other auth_scheme=bearer origin=yes referer=no",
        (_, "bearer", false, true) => "request=other auth_scheme=bearer origin=no referer=yes",
        (_, "bearer", false, false) => "request=other auth_scheme=bearer origin=no referer=no",
        (_, "basic", _, _) => "request=other auth_scheme=basic",
        (_, "other", _, _) => "request=other auth_scheme=other",
        _ => "request=other auth_scheme=none",
    }
}
