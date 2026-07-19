use std::{
    collections::BTreeMap,
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, TimeZone, Utc};
use tauri::{
    AppHandle, Manager, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent,
};
use tokio::sync::mpsc;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL, ICoreWebView2HttpRequestHeaders,
};
use webview2_com::{
    ExecuteScriptCompletedHandler, GetCookiesCompletedHandler, WebResourceRequestedEventHandler,
    take_pwstr,
};
use windows::core::{HSTRING, Interface, PWSTR, w};

use crate::{
    api::error::{ApiError, PublicError, PublicErrorCode},
    app::AppServices,
};

const LOGIN_LABEL: &str = "login";
const LOGIN_URL: &str = "https://www.sevnx.one/login?redirect=/dashboard";
const COOKIE_ORIGIN_APEX: &str = "https://sevnx.one/";
const COOKIE_ORIGIN_WWW: &str = "https://www.sevnx.one/";
const MAX_BEARER_TOKEN_LENGTH: usize = 8 * 1024;
const MAX_COOKIE_HEADER_LENGTH: usize = 64 * 1024;
const API_REQUEST_FILTER_APEX: &str = "https://sevnx.one/api/v1/*";
const API_REQUEST_FILTER_WWW: &str = "https://www.sevnx.one/api/v1/*";
const DASHBOARD_CREDENTIAL_CAPTURE_TIMEOUT: Duration = Duration::from_secs(45);

// This script deliberately does not enumerate storage. It reads only the
// explicit keys below and returns a single value to Rust for read-only
// `/auth/me` validation. It runs only after the WebView's current origin is
// confirmed as the first-party SevnX site.
const WHITELISTED_STORAGE_TOKEN_SCRIPT: &str = r#"
(() => {
  const values = [];
  const keys = [
    'access_token',
    'accessToken',
    'auth_token',
    'authToken',
    'user_token',
    'userToken',
    'sevnx_token',
    'sevnxToken',
    'session_token',
    'sessionToken',
    'token',
    'jwt',
    'jwtToken'
  ];
  const stores = [window.localStorage, window.sessionStorage];
  for (const store of stores) {
    try {
      for (const key of keys) {
        const value = store.getItem(key);
        if (typeof value === 'string' && value.length > 0) {
          values.push(value);
        }
      }
    } catch (_) {
      // Storage can be unavailable during navigation; the next poll retries.
    }
  }
  return values;
})()
"#;

struct LoginCredentials {
    cookie_header: Option<String>,
    authorization_header: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Default)]
struct LoginRequestCredentialObserver {
    inner: Arc<Mutex<ObservedRequestCredentials>>,
}

#[derive(Clone, Default)]
struct ObservedRequestCredentials {
    generation: u64,
    cookie_header: Option<String>,
    authorization_header: Option<String>,
    api_origin: Option<&'static str>,
    request_kind: &'static str,
    request_path: String,
    authorization_scheme: &'static str,
    has_origin: bool,
    has_referer: bool,
    request_context: crate::auth::RequestContext,
}

impl LoginRequestCredentialObserver {
    fn observe(
        &self,
        cookie_header: Option<String>,
        authorization_header: Option<String>,
        api_origin: Option<&'static str>,
        request_kind: &'static str,
        request_path: String,
        authorization_scheme: &'static str,
        has_origin: bool,
        has_referer: bool,
        request_context: crate::auth::RequestContext,
    ) {
        if cookie_header.is_none() && authorization_header.is_none() {
            return;
        }
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if inner.cookie_header == cookie_header
            && inner.authorization_header == authorization_header
            && inner.api_origin == api_origin
            && inner.request_kind == request_kind
            && inner.request_path == request_path
            && inner.authorization_scheme == authorization_scheme
            && inner.has_origin == has_origin
            && inner.has_referer == has_referer
            && inner.request_context.user_agent == request_context.user_agent
            && inner.request_context.accept == request_context.accept
            && inner.request_context.accept_language == request_context.accept_language
            && inner.request_context.sec_fetch_dest == request_context.sec_fetch_dest
            && inner.request_context.sec_fetch_mode == request_context.sec_fetch_mode
            && inner.request_context.sec_fetch_site == request_context.sec_fetch_site
        {
            return;
        }
        inner.generation = inner.generation.wrapping_add(1);
        inner.cookie_header = cookie_header;
        inner.authorization_header = authorization_header;
        inner.api_origin = api_origin;
        inner.request_kind = request_kind;
        inner.request_path = request_path;
        inner.authorization_scheme = authorization_scheme;
        inner.has_origin = has_origin;
        inner.has_referer = has_referer;
        inner.request_context = request_context;
    }

    fn snapshot(&self) -> ObservedRequestCredentials {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

struct CookieCapture {
    pairs: BTreeMap<String, String>,
    expires_at: Option<DateTime<Utc>>,
}

struct LoginCredentialCapture {
    credentials: Option<LoginCredentials>,
    bearer_candidates: Vec<String>,
    cookie_capture_failed: bool,
    storage_capture_failed: bool,
}

#[derive(Default)]
struct LoginProbeLogState {
    cookie_capture_failed: bool,
    storage_capture_failed: bool,
    candidate_missing: bool,
    candidate_source: Option<(bool, bool)>,
    validation_failure: Option<&'static str>,
}

pub async fn open_login_window(app: AppHandle, attempt: u64) -> Result<(), PublicError> {
    let services = app.state::<AppServices>();
    if let Some(existing) = app.get_webview_window(LOGIN_LABEL) {
        existing.show().map_err(window_error)?;
        existing.set_focus().map_err(window_error)?;
        if services.reuse_login_profile(attempt).is_some() {
            tauri::async_runtime::spawn(poll_login_session(
                app.clone(),
                attempt,
                LoginRequestCredentialObserver::default(),
            ));
        }
        return Ok(());
    }

    let profile = new_login_profile(services.paths().webview_profile_dir(), attempt);
    let request_observer = LoginRequestCredentialObserver::default();
    let builder_app = app.clone();
    let build_profile = profile.clone();
    let build_observer = request_observer.clone();
    let build_result = match tokio::task::spawn_blocking(move || {
        build_login_window(builder_app, build_profile, build_observer)
    })
    .await
    {
        Ok(result) => result,
        Err(_) => {
            cleanup_login_profile(profile);
            return Err(PublicError::new(
                PublicErrorCode::Internal,
                "无法创建登录窗口",
            ));
        }
    };
    if let Err(error) = build_result {
        cleanup_login_profile(profile);
        return Err(error);
    }

    services.register_login_profile(attempt, profile.clone());
    tauri::async_runtime::spawn(poll_login_session(app.clone(), attempt, request_observer));
    Ok(())
}

fn build_login_window(
    app: AppHandle,
    profile: PathBuf,
    request_observer: LoginRequestCredentialObserver,
) -> Result<(), PublicError> {
    // This data directory belongs only to the short-lived login WebView2. A
    // successful flow removes it after copying the validated credentials into
    // the DPAPI store, so browser profile data is not another persistence path.
    fs::create_dir_all(&profile).map_err(|_| window_error(()))?;

    let login_url = Url::parse(LOGIN_URL)
        .map_err(|_| PublicError::new(PublicErrorCode::Internal, "登录地址无效"))?;
    let window = WebviewWindowBuilder::new(&app, LOGIN_LABEL, WebviewUrl::External(login_url))
        .title("SevnX Monitor 登录")
        .inner_size(1180.0, 860.0)
        .min_inner_size(800.0, 600.0)
        .center()
        .resizable(true)
        .always_on_top(true)
        .data_directory(profile.clone())
        .on_navigation({
            let app = app.clone();
            move |url| allow_login_navigation(&app, url)
        })
        .build()
        .map_err(window_error)?;
    register_api_request_observer(&window, request_observer)?;
    Ok(())
}

fn register_api_request_observer(
    window: &WebviewWindow,
    observer: LoginRequestCredentialObserver,
) -> Result<(), PublicError> {
    let (registration_sender, registration_receiver) = std::sync::mpsc::sync_channel(1);
    window
        .with_webview(move |webview| {
            let result = (|| unsafe {
                let core = webview.controller().CoreWebView2()?;
                for filter in [API_REQUEST_FILTER_APEX, API_REQUEST_FILTER_WWW] {
                    core.AddWebResourceRequestedFilter(
                        &HSTRING::from(filter),
                        COREWEBVIEW2_WEB_RESOURCE_CONTEXT_ALL,
                    )?;
                }
                let handler =
                    WebResourceRequestedEventHandler::create(Box::new(move |_, event_args| {
                        let Some(event_args) = event_args else {
                            return Ok(());
                        };
                        let request = event_args.Request()?;
                        let mut uri = PWSTR::null();
                        request.Uri(&mut uri)?;
                        let uri = take_pwstr(uri);
                        if !is_login_validation_request_uri(&uri) {
                            return Ok(());
                        }
                        let api_origin = observed_api_origin(&uri);
                        let headers = request.Headers()?;
                        let cookie_header = read_request_header(&headers, w!("Cookie"))
                            .filter(|value| value.len() <= MAX_COOKIE_HEADER_LENGTH);
                        let authorization_header =
                            read_request_header(&headers, w!("Authorization"))
                                .filter(|value| value.len() <= MAX_BEARER_TOKEN_LENGTH);
                        let request_kind = observed_request_kind(&uri);
                        let request_path = observed_api_path(&uri).unwrap_or_default();
                        let authorization_scheme =
                            authorization_scheme(authorization_header.as_deref());
                        let has_origin = read_request_header(&headers, w!("Origin")).is_some();
                        let has_referer = read_request_header(&headers, w!("Referer")).is_some();
                        let request_context = crate::auth::RequestContext {
                            api_origin: api_origin.map(str::to_owned),
                            user_agent: read_request_header(&headers, w!("User-Agent")),
                            accept: read_request_header(&headers, w!("Accept")),
                            accept_language: read_request_header(&headers, w!("Accept-Language")),
                            sec_fetch_dest: read_request_header(&headers, w!("Sec-Fetch-Dest")),
                            sec_fetch_mode: read_request_header(&headers, w!("Sec-Fetch-Mode")),
                            sec_fetch_site: read_request_header(&headers, w!("Sec-Fetch-Site")),
                        };
                        observer.observe(
                            cookie_header,
                            authorization_header,
                            api_origin,
                            request_kind,
                            request_path,
                            authorization_scheme,
                            has_origin,
                            has_referer,
                            request_context,
                        );
                        Ok(())
                    }));
                let mut token = 0_i64;
                core.add_WebResourceRequested(&handler, &mut token)
            })();
            let _ = registration_sender.send(result.is_ok());
        })
        .map_err(window_error)?;
    match registration_receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(true) => Ok(()),
        _ => Err(window_error(())),
    }
}

fn read_request_header(
    headers: &ICoreWebView2HttpRequestHeaders,
    name: windows::core::PCWSTR,
) -> Option<String> {
    let mut value = PWSTR::null();
    if unsafe { headers.GetHeader(name, &mut value) }.is_err() {
        return None;
    }
    let value = take_pwstr(value);
    let value = value.trim();
    (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_owned())
}

fn is_allowed_credential_request_uri(uri: &str) -> bool {
    observed_api_origin(uri).is_some()
}

fn is_login_validation_request_uri(uri: &str) -> bool {
    let Ok(url) = Url::parse(uri) else {
        return false;
    };
    is_allowed_credential_request_uri(uri) && url.path() == "/api/v1/usage/dashboard/stats"
}

fn observed_api_origin(uri: &str) -> Option<&'static str> {
    let Ok(url) = Url::parse(uri) else {
        return None;
    };
    if !url.path().starts_with("/api/") {
        return None;
    }
    match url.host_str().map(str::to_ascii_lowercase).as_deref() {
        Some("sevnx.one") => Some("https://sevnx.one/api/v1"),
        Some("www.sevnx.one") => Some("https://www.sevnx.one/api/v1"),
        _ => None,
    }
}

fn observed_request_kind(uri: &str) -> &'static str {
    let Ok(url) = Url::parse(uri) else {
        return "other_api";
    };
    match url.path() {
        "/api/v1/auth/me" => "auth_me",
        path if path.starts_with("/api/v1/auth/") => "auth_other",
        "/api/v1/usage/dashboard/stats" => "dashboard_stats",
        "/api/v1/usage/dashboard/models" => "dashboard_models",
        "/api/v1/usage/dashboard/snapshot-v2" => "dashboard_snapshot",
        "/api/v1/usage/stats" => "usage_stats",
        path if path.starts_with("/api/v1/usage/") => "usage_other",
        path if path.starts_with("/api/v1/user") => "user",
        path if path.starts_with("/api/v1/api-key") || path.starts_with("/api/v1/keys") => {
            "api_keys"
        }
        path if path.starts_with("/api/v1/balance") => "balance",
        _ => "other_api",
    }
}

fn observed_api_path(uri: &str) -> Option<String> {
    let url = Url::parse(uri).ok()?;
    let path = url.path();
    (path.starts_with("/api/")
        && path.len() <= 160
        && path.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '/' | '-' | '_')
        }))
    .then(|| path.to_owned())
}

fn authorization_scheme(value: Option<&str>) -> &'static str {
    match value.map(str::trim) {
        Some(value)
            if value
                .get(..7)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("bearer ")) =>
        {
            "bearer"
        }
        Some(value)
            if value
                .get(..6)
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case("basic ")) =>
        {
            "basic"
        }
        Some(_) => "other",
        None => "none",
    }
}

fn is_dashboard_url(url: &Url) -> bool {
    is_first_party_sevnx_url(url) && url.path().starts_with("/dashboard")
}

fn new_login_profile(parent: PathBuf, attempt: u64) -> PathBuf {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    parent.join(format!(
        "login-{}-{attempt}-{timestamp}",
        std::process::id()
    ))
}

/// SevnX performs its post-login navigation in the dashboard SPA, so a page
/// load callback is not a reliable completion signal. Follow the verified RAW
/// pattern instead: poll the application's own WebView2 CookieManager and
/// accept the session only after the read-only `/auth/me` endpoint succeeds.
/// No candidate cookie is installed, persisted, logged, or sent to the UI
/// before that validation succeeds.
async fn poll_login_session(
    app: AppHandle,
    attempt: u64,
    request_observer: LoginRequestCredentialObserver,
) {
    let mut log_state = LoginProbeLogState::default();
    let mut observed_request_generation = 0_u64;
    let mut dashboard_capture_deadline = None;
    let mut dashboard_detected_logged = false;
    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let services = app.state::<AppServices>();
        let Some(window) = app.get_webview_window(LOGIN_LABEL) else {
            if let Some(profile) = services.take_login_profile_for_attempt(attempt) {
                cleanup_login_profile(profile);
            }
            return;
        };
        if !services.is_current_login_attempt(attempt).await {
            return;
        }
        if !window.is_visible().unwrap_or(false) {
            return;
        }
        if window.url().ok().is_some_and(|url| is_dashboard_url(&url)) {
            if !dashboard_detected_logged {
                services.record_login_dashboard_detected();
                dashboard_detected_logged = true;
            }
            let deadline = dashboard_capture_deadline
                .get_or_insert_with(|| Instant::now() + DASHBOARD_CREDENTIAL_CAPTURE_TIMEOUT);
            if Instant::now() >= *deadline {
                fail_login_session(
                    &app,
                    &services,
                    attempt,
                    PublicError::new(
                        PublicErrorCode::AuthenticationRequired,
                        "已完成网页登录，但未能读取可验证的登录凭证，请关闭后重试",
                    ),
                )
                .await;
                return;
            }
        }

        let observed_request = request_observer.snapshot();
        if observed_request.generation != 0
            && observed_request.generation != observed_request_generation
        {
            observed_request_generation = observed_request.generation;
            let has_cookie = observed_request.cookie_header.is_some();
            let has_authorization = observed_request.authorization_header.is_some();
            services.record_login_request_credentials_observed(has_cookie, has_authorization);
            services.record_login_request_context(
                observed_request.request_kind,
                observed_request.authorization_scheme,
                observed_request.has_origin,
                observed_request.has_referer,
            );
            services.record_login_request_path(&observed_request.request_path);
            if !services.login_is_pending() || !services.begin_login_capture() {
                return;
            }
            if !services.is_current_login_attempt(attempt).await {
                services.finish_login_capture();
                return;
            }
            match services
                .client()
                .validate_login_candidate(
                    observed_request.cookie_header.as_deref(),
                    observed_request.authorization_header.as_deref(),
                    observed_request.api_origin,
                    Some(&observed_request.request_context),
                )
                .await
            {
                Ok(()) => {
                    services
                        .record_login_request_credentials_validated(has_cookie, has_authorization);
                    if !services.is_current_login_attempt(attempt).await {
                        services.finish_login_capture();
                        return;
                    }
                    match services
                        .complete_login_from_verified_credentials(
                            attempt,
                            observed_request.cookie_header,
                            observed_request.authorization_header,
                            None,
                            observed_request.request_context,
                        )
                        .await
                    {
                        Ok(Some(_)) => {
                            crate::services::balance_alert::evaluate(&app, &services).await;
                            crate::commands::emit_app_state(&app, &services);
                            let profile = services.take_login_profile_for_attempt(attempt);
                            if let Some(login) = app.get_webview_window(LOGIN_LABEL) {
                                let _ = login.close();
                            }
                            if let Some(profile) = profile {
                                cleanup_login_profile(profile);
                            }
                            services.finish_login_capture();
                            return;
                        }
                        Ok(None) => {
                            crate::commands::emit_app_state(&app, &services);
                            services.finish_login_capture();
                            return;
                        }
                        Err(error) => {
                            fail_login_session(&app, &services, attempt, error).await;
                            return;
                        }
                    }
                }
                Err(error) => {
                    services.record_login_request_credentials_validation_failure(&error);
                    services.finish_login_capture();
                }
            }
        }

        let LoginCredentialCapture {
            credentials,
            bearer_candidates,
            cookie_capture_failed,
            storage_capture_failed,
        } = capture_login_credentials_v2(window).await;
        if cookie_capture_failed && !log_state.cookie_capture_failed {
            services.record_login_cookie_capture_failure();
        }
        log_state.cookie_capture_failed = cookie_capture_failed;
        if storage_capture_failed && !log_state.storage_capture_failed {
            services.record_login_storage_capture_failure();
        }
        log_state.storage_capture_failed = storage_capture_failed;

        let Some(mut credentials) = credentials else {
            if !log_state.candidate_missing {
                services.record_login_candidate_missing();
            }
            log_state.candidate_missing = true;
            continue;
        };
        log_state.candidate_missing = false;

        let has_cookie = credentials.cookie_header.is_some();
        let has_bearer = !bearer_candidates.is_empty();
        if log_state.candidate_source != Some((has_cookie, has_bearer)) {
            services.record_login_candidate_observed(has_cookie, has_bearer);
            log_state.candidate_source = Some((has_cookie, has_bearer));
            log_state.validation_failure = None;
        }

        if !services.login_is_pending() || !services.begin_login_capture() {
            return;
        }
        if !services.is_current_login_attempt(attempt).await {
            services.finish_login_capture();
            return;
        }

        let validated_source = validate_login_credentials(
            services.client().as_ref(),
            &credentials,
            &bearer_candidates,
        )
        .await;
        let (validated_cookie, validated_bearer_index) = match validated_source {
            Ok(source) => source,
            Err(error) => {
                if log_state.validation_failure.is_none() {
                    services.record_login_candidate_validation_failure(&error);
                    log_state.validation_failure = Some("recorded");
                }
                services.finish_login_capture();
                continue;
            }
        };
        services
            .record_login_candidate_validated(validated_cookie, validated_bearer_index.is_some());
        if !validated_cookie {
            credentials.cookie_header = None;
            credentials.expires_at = None;
        }
        credentials.authorization_header = validated_bearer_index
            .and_then(|index| bearer_candidates.into_iter().nth(index))
            .map(|token| format!("Bearer {token}"));

        // The user might have closed the login window while `/auth/me` was
        // running. Do not let that stale success recreate a cancelled session.
        if !services.is_current_login_attempt(attempt).await {
            services.finish_login_capture();
            return;
        }

        match services
            .complete_login_from_verified_credentials(
                attempt,
                credentials.cookie_header,
                credentials.authorization_header,
                credentials.expires_at,
                crate::auth::RequestContext::default(),
            )
            .await
        {
            Ok(Some(_)) => {
                crate::services::balance_alert::evaluate(&app, &services).await;
                crate::commands::emit_app_state(&app, &services);
                let profile = services.take_login_profile_for_attempt(attempt);
                if let Some(login) = app.get_webview_window(LOGIN_LABEL) {
                    let _ = login.close();
                }
                if let Some(profile) = profile {
                    cleanup_login_profile(profile);
                }
                services.finish_login_capture();
                return;
            }
            Ok(None) => {
                // A synchronous CloseRequested can win the commit race after
                // this poll started. Publish the cancellation/restored state
                // even when its window-event task already observed it.
                crate::commands::emit_app_state(&app, &services);
                services.finish_login_capture();
                return;
            }
            Err(error) => {
                fail_login_session(&app, &services, attempt, error).await;
                return;
            }
        }
    }
}

async fn fail_login_session(
    app: &AppHandle,
    services: &AppServices,
    attempt: u64,
    error: PublicError,
) {
    if !services.fail_login_capture(attempt, error).await {
        return;
    }
    crate::commands::emit_app_state(app, services);
    let profile = services.take_login_profile_for_attempt(attempt);
    if let Some(login) = app.get_webview_window(LOGIN_LABEL) {
        let _ = login.close();
    }
    if let Some(profile) = profile {
        cleanup_login_profile(profile);
    }
}

async fn validate_login_credentials(
    client: &crate::api::client::SevnxApiClient,
    credentials: &LoginCredentials,
    bearer_candidates: &[String],
) -> Result<(bool, Option<usize>), ApiError> {
    let cookie = credentials.cookie_header.as_deref();
    let mut last_auth_error = ApiError::MissingCredentials;

    if cookie.is_some() {
        for (index, bearer) in bearer_candidates.iter().enumerate() {
            match client
                .validate_login_candidate(cookie, Some(&format!("Bearer {bearer}")), None, None)
                .await
            {
                Ok(()) => return Ok((true, Some(index))),
                Err(error) if error.is_auth_invalid() => last_auth_error = error,
                Err(error) => return Err(error),
            }
        }
    }

    for (index, bearer) in bearer_candidates.iter().enumerate() {
        match client
            .validate_login_candidate(None, Some(&format!("Bearer {bearer}")), None, None)
            .await
        {
            Ok(()) => return Ok((false, Some(index))),
            Err(error) if error.is_auth_invalid() => last_auth_error = error,
            Err(error) => return Err(error),
        }
    }

    if cookie.is_some() {
        return client
            .validate_login_candidate(cookie, None, None, None)
            .await
            .map(|()| (true, None));
    }
    Err(last_auth_error)
}

/// The login WebView may be closed by the user before navigation or cookie
/// capture completes. Return the main app to a usable state immediately and
/// remove this short-lived browser profile after WebView2 releases it.
pub fn handle_window_event(app: AppHandle, label: &str, event: &WindowEvent) {
    if label != LOGIN_LABEL
        || !matches!(
            event,
            WindowEvent::CloseRequested { .. } | WindowEvent::Destroyed
        )
    {
        return;
    }
    // CloseRequested happens before the Tauri label can be reused. Taking the
    // profile here makes a rapid close/re-open independent of WebView2's
    // asynchronous directory release. Destroyed remains a cancellation
    // fallback; its polling task cleans up the profile if no close event ran.
    let services = app.state::<AppServices>();
    let cancelled_attempt = services.cancel_login_attempt_from_window();
    let profile = matches!(event, WindowEvent::CloseRequested { .. })
        .then(|| services.take_active_login_profile())
        .flatten();
    tauri::async_runtime::spawn(async move {
        let services = app.state::<AppServices>();
        let cancelled = match cancelled_attempt {
            Some(attempt) => services.cancel_login_window(attempt).await,
            None => false,
        };
        if cancelled {
            crate::commands::emit_app_state(&app, &services);
        }
        if let Some(profile) = profile {
            cleanup_login_profile(profile);
        }
    });
}

fn allow_login_navigation(app: &AppHandle, url: &Url) -> bool {
    let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
    let allowed = matches!(host.as_str(), "sevnx.one" | "www.sevnx.one")
        || host.ends_with(".sevnx.one")
        // Explicit identity-provider exceptions. No arbitrary external URL is
        // allowed to remain inside the application's WebView2 profile.
        || matches!(host.as_str(), "accounts.google.com" | "github.com");
    if !allowed && matches!(url.scheme(), "http" | "https") {
        // A regular external link belongs to the user's default browser and
        // never participates in session extraction.
        let _ = tauri_plugin_opener::OpenerExt::opener(app).open_url(url.as_str(), None::<&str>);
    }
    allowed
}

#[allow(dead_code)]
async fn capture_login_credentials_legacy(
    window: WebviewWindow,
) -> Result<LoginCredentials, PublicError> {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    window
        .with_webview(move |webview| {
            let result = (|| unsafe {
                let core = webview.controller().CoreWebView2()?;
                let core =
                    core.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2>()?;
                let cookie_manager = core.CookieManager()?;
                let callback_sender = sender.clone();
                let handler = GetCookiesCompletedHandler::create(Box::new(move |status, list| {
                    let captured = if status.is_err() {
                        Err(PublicError::new(
                            PublicErrorCode::AuthenticationRequired,
                            "未能读取登录凭证",
                        ))
                    } else {
                        list.as_ref()
                            .ok_or_else(|| {
                                PublicError::new(
                                    PublicErrorCode::AuthenticationRequired,
                                    "未检测到登录凭证",
                                )
                            })
                            .and_then(read_cookie_list_legacy)
                    };
                    let _ = callback_sender.send(captured);
                    Ok(())
                }));
                cookie_manager.GetCookies(w!("https://www.sevnx.one/"), &handler)
            })();
            if result.is_err() {
                let _ = sender.send(Err(PublicError::new(
                    PublicErrorCode::AuthenticationRequired,
                    "未能读取登录凭证",
                )));
            }
        })
        .map_err(window_error)?;

    tokio::time::timeout(Duration::from_secs(10), receiver.recv())
        .await
        .map_err(|_| PublicError::new(PublicErrorCode::Timeout, "读取登录凭证超时"))?
        .ok_or_else(|| PublicError::new(PublicErrorCode::Internal, "登录窗口已关闭"))?
}

#[allow(dead_code)]
fn read_cookie_list_legacy(
    list: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2CookieList,
) -> Result<LoginCredentials, PublicError> {
    let mut count = 0;
    unsafe {
        list.Count(&mut count).map_err(|_| {
            PublicError::new(PublicErrorCode::AuthenticationRequired, "未检测到登录凭证")
        })?;
    }
    let mut pairs = Vec::with_capacity(count as usize);
    let mut expires_at = None;
    for index in 0..count {
        let cookie = unsafe {
            list.GetValueAtIndex(index).map_err(|_| {
                PublicError::new(PublicErrorCode::AuthenticationRequired, "未能读取登录凭证")
            })?
        };
        let mut name = PWSTR::null();
        let mut value = PWSTR::null();
        unsafe {
            cookie.Name(&mut name).map_err(|_| {
                PublicError::new(PublicErrorCode::AuthenticationRequired, "未能读取登录凭证")
            })?;
            cookie.Value(&mut value).map_err(|_| {
                PublicError::new(PublicErrorCode::AuthenticationRequired, "未能读取登录凭证")
            })?;
        }
        let name = take_pwstr(name);
        let value = take_pwstr(value);
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let mut expires = 0_f64;
        if unsafe { cookie.Expires(&mut expires) }.is_ok() && expires.is_finite() && expires > 0.0 {
            let candidate = Utc.timestamp_opt(expires.floor() as i64, 0).single();
            if candidate.is_some() {
                expires_at = candidate.or(expires_at);
            }
        }
        pairs.push(format!("{name}={value}"));
    }
    if pairs.is_empty() {
        return Err(PublicError::new(
            PublicErrorCode::AuthenticationRequired,
            "未检测到登录凭证",
        ));
    }
    Ok(LoginCredentials {
        cookie_header: Some(pairs.join("; ")),
        authorization_header: None,
        expires_at,
    })
}

async fn capture_login_credentials_v2(window: WebviewWindow) -> LoginCredentialCapture {
    let apex = capture_cookies_for_origin(window.clone(), COOKIE_ORIGIN_APEX).await;
    let www = capture_cookies_for_origin(window.clone(), COOKIE_ORIGIN_WWW).await;
    let cookie_capture_failed = apex.is_err() || www.is_err();
    let mut pairs = BTreeMap::new();
    let mut expires_at = None;
    for capture in [apex, www].into_iter().flatten() {
        pairs.extend(capture.pairs);
        if let Some(candidate) = capture.expires_at {
            expires_at =
                Some(expires_at.map_or(candidate, |current: DateTime<Utc>| current.min(candidate)));
        }
    }

    let storage = match window.url() {
        Ok(url) if is_first_party_sevnx_url(&url) => capture_storage_tokens(window).await,
        _ => Ok(Vec::new()),
    };
    let storage_capture_failed = storage.is_err();
    let bearer_candidates = storage.unwrap_or_default();
    let cookie_header = (!pairs.is_empty()).then(|| {
        pairs
            .into_iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("; ")
    });
    let credentials =
        (cookie_header.is_some() || !bearer_candidates.is_empty()).then_some(LoginCredentials {
            cookie_header,
            authorization_header: None,
            expires_at,
        });
    LoginCredentialCapture {
        credentials,
        bearer_candidates,
        cookie_capture_failed,
        storage_capture_failed,
    }
}

fn is_first_party_sevnx_url(url: &Url) -> bool {
    url.scheme() == "https"
        && matches!(
            url.host_str().map(str::to_ascii_lowercase).as_deref(),
            Some("sevnx.one" | "www.sevnx.one")
        )
}

async fn capture_cookies_for_origin(
    window: WebviewWindow,
    origin: &'static str,
) -> Result<CookieCapture, PublicError> {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    window
        .with_webview(move |webview| {
            let result = (|| unsafe {
                let core = webview.controller().CoreWebView2()?;
                let core =
                    core.cast::<webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2_2>()?;
                let cookie_manager = core.CookieManager()?;
                let callback_sender = sender.clone();
                let handler = GetCookiesCompletedHandler::create(Box::new(move |status, list| {
                    let captured = if status.is_err() {
                        Err(credential_capture_error())
                    } else {
                        list.as_ref()
                            .ok_or_else(credential_capture_error)
                            .and_then(read_cookie_list_v2)
                    };
                    let _ = callback_sender.send(captured);
                    Ok(())
                }));
                let uri = HSTRING::from(origin);
                cookie_manager.GetCookies(&uri, &handler)
            })();
            if result.is_err() {
                let _ = sender.send(Err(credential_capture_error()));
            }
        })
        .map_err(window_error)?;

    tokio::time::timeout(Duration::from_secs(10), receiver.recv())
        .await
        .map_err(|_| PublicError::new(PublicErrorCode::Timeout, "读取登录凭证超时"))?
        .ok_or_else(|| PublicError::new(PublicErrorCode::Internal, "登录窗口已关闭"))?
}

async fn capture_storage_tokens(window: WebviewWindow) -> Result<Vec<String>, PublicError> {
    let (sender, mut receiver) = mpsc::unbounded_channel();
    window
        .with_webview(move |webview| {
            let result = (|| unsafe {
                let core = webview.controller().CoreWebView2()?;
                let callback_sender = sender.clone();
                let handler =
                    ExecuteScriptCompletedHandler::create(Box::new(move |status, result| {
                        let captured = if status.is_err() {
                            Err(credential_capture_error())
                        } else {
                            Ok(parse_storage_token_results(&result))
                        };
                        let _ = callback_sender.send(captured);
                        Ok(())
                    }));
                let script = HSTRING::from(WHITELISTED_STORAGE_TOKEN_SCRIPT);
                core.ExecuteScript(&script, &handler)
            })();
            if result.is_err() {
                let _ = sender.send(Err(credential_capture_error()));
            }
        })
        .map_err(window_error)?;

    tokio::time::timeout(Duration::from_secs(5), receiver.recv())
        .await
        .map_err(|_| PublicError::new(PublicErrorCode::Timeout, "读取登录凭证超时"))?
        .ok_or_else(|| PublicError::new(PublicErrorCode::Internal, "登录窗口已关闭"))?
}

fn parse_storage_token_results(result: &str) -> Vec<String> {
    let stored: Vec<String> = serde_json::from_str(result).unwrap_or_default();
    let mut tokens = Vec::new();
    for value in stored {
        if let Some(token) = normalize_storage_token(value.trim()) {
            if !tokens.contains(&token) {
                tokens.push(token);
            }
        }
    }
    tokens
}

fn normalize_storage_token(raw: &str) -> Option<String> {
    let token = match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::String(value)) => value,
        Ok(serde_json::Value::Object(object)) => [
            "value",
            "access_token",
            "accessToken",
            "auth_token",
            "authToken",
            "token",
        ]
        .into_iter()
        .find_map(|key| object.get(key).and_then(serde_json::Value::as_str))
        .map(str::to_owned)?,
        _ => raw.to_owned(),
    };
    let token = token
        .strip_prefix("Bearer ")
        .or_else(|| token.strip_prefix("bearer "))
        .unwrap_or(&token)
        .trim();
    (!token.is_empty()
        && token.len() <= MAX_BEARER_TOKEN_LENGTH
        && !token.chars().any(char::is_control))
    .then(|| token.to_owned())
}

fn read_cookie_list_v2(
    list: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2CookieList,
) -> Result<CookieCapture, PublicError> {
    let mut count = 0;
    unsafe {
        list.Count(&mut count)
            .map_err(|_| credential_capture_error())?;
    }
    let mut pairs = BTreeMap::new();
    let mut expires_at = None;
    for index in 0..count {
        let cookie = unsafe {
            list.GetValueAtIndex(index)
                .map_err(|_| credential_capture_error())?
        };
        let mut name = PWSTR::null();
        let mut value = PWSTR::null();
        unsafe {
            cookie
                .Name(&mut name)
                .map_err(|_| credential_capture_error())?;
            cookie
                .Value(&mut value)
                .map_err(|_| credential_capture_error())?;
        }
        let name = take_pwstr(name);
        let value = take_pwstr(value);
        if name.is_empty() || value.is_empty() {
            continue;
        }
        let mut expires = 0_f64;
        if unsafe { cookie.Expires(&mut expires) }.is_ok() && expires.is_finite() && expires > 0.0 {
            if let Some(candidate) = Utc.timestamp_opt(expires.floor() as i64, 0).single() {
                expires_at = Some(
                    expires_at.map_or(candidate, |current: DateTime<Utc>| current.min(candidate)),
                );
            }
        }
        pairs.insert(name, value);
    }
    Ok(CookieCapture { pairs, expires_at })
}

fn credential_capture_error() -> PublicError {
    PublicError::new(PublicErrorCode::AuthenticationRequired, "未能读取登录凭证")
}

fn cleanup_login_profile(profile: PathBuf) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_millis(750)).await;
        let _ = fs::remove_dir_all(profile);
    });
}

#[cfg(test)]
mod tests {
    use super::{
        WHITELISTED_STORAGE_TOKEN_SCRIPT, is_allowed_credential_request_uri, is_dashboard_url,
        is_first_party_sevnx_url, is_login_validation_request_uri, normalize_storage_token,
        observed_api_origin, parse_storage_token_results,
    };
    use tauri::Url;

    #[test]
    fn storage_token_parser_accepts_raw_and_wrapped_values() {
        assert_eq!(
            normalize_storage_token("Bearer test-token").as_deref(),
            Some("test-token")
        );
        assert_eq!(
            normalize_storage_token(r#"{"value":"wrapped-token"}"#).as_deref(),
            Some("wrapped-token")
        );
        assert_eq!(
            parse_storage_token_results(
                r#"["{\"value\":\"script-token\"}","second-token","second-token"]"#
            ),
            vec!["script-token".to_string(), "second-token".to_string()]
        );
    }

    #[test]
    fn storage_token_parser_rejects_control_characters() {
        assert!(normalize_storage_token("bad\ntoken").is_none());
    }

    #[test]
    fn storage_script_reads_only_explicit_whitelist_keys() {
        assert!(WHITELISTED_STORAGE_TOKEN_SCRIPT.contains("'userToken'"));
        assert!(!WHITELISTED_STORAGE_TOKEN_SCRIPT.contains("Object.keys"));
        assert!(!WHITELISTED_STORAGE_TOKEN_SCRIPT.contains(".length]"));
    }

    #[test]
    fn storage_capture_is_limited_to_first_party_origins() {
        assert!(is_first_party_sevnx_url(
            &Url::parse("https://www.sevnx.one/dashboard").expect("valid URL")
        ));
        assert!(is_first_party_sevnx_url(
            &Url::parse("https://sevnx.one/dashboard").expect("valid URL")
        ));
        assert!(!is_first_party_sevnx_url(
            &Url::parse("https://sevnx.one.example/dashboard").expect("valid URL")
        ));
    }

    #[test]
    fn request_observer_accepts_first_party_api_routes_only() {
        assert!(is_allowed_credential_request_uri(
            "https://www.sevnx.one/api/v1/auth/session"
        ));
        assert!(is_login_validation_request_uri(
            "https://www.sevnx.one/api/v1/usage/dashboard/stats"
        ));
        assert!(!is_login_validation_request_uri(
            "https://www.sevnx.one/api/v1/announcements"
        ));
        assert_eq!(
            observed_api_origin("https://sevnx.one/api/v1/auth/me"),
            Some("https://sevnx.one/api/v1")
        );
        assert_eq!(
            observed_api_origin("https://www.sevnx.one/api/v1/auth/me"),
            Some("https://www.sevnx.one/api/v1")
        );
        assert!(!is_allowed_credential_request_uri(
            "https://www.sevnx.one/dashboard"
        ));
        assert!(!is_allowed_credential_request_uri(
            "https://sevnx.one.example/api/v1/auth/me"
        ));
    }

    #[test]
    fn dashboard_detection_requires_a_first_party_dashboard_route() {
        assert!(is_dashboard_url(
            &Url::parse("https://www.sevnx.one/dashboard?tab=overview").expect("valid URL")
        ));
        assert!(!is_dashboard_url(
            &Url::parse("https://www.sevnx.one/login?redirect=/dashboard").expect("valid URL")
        ));
    }
}

fn window_error<T>(_error: T) -> PublicError {
    PublicError::new(PublicErrorCode::Internal, "无法创建或控制登录窗口")
}
