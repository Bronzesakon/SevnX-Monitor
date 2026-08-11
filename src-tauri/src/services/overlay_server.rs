//! Local loopback HTTP server that exposes a whitelist-shaped SevnX snapshot
//! to the JS injected into Codex.
//!
//! Security model (design §12): the server binds `127.0.0.1` on a random port
//! and requires a per-process random bearer token. The token lives only in
//! memory and never persists. Only the three bar metrics (+ auth + timestamp)
//! and the explicit detail fields cross the boundary; credentials and raw API
//! bodies never leave the Tauri side.
//!
//! Delivery model (design §5/§8): `/overlay` returns the compact bar payload,
//! `/overlay/detail` returns the full small-window payload on demand, and
//! `/overlay/poll` long-polls a monotonic `rev` so injected clients only
//! receive a payload once the dashboard actually changed.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Body,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::model::{AppSnapshot, AppState, AuthStatus};
use crate::services::logging::SafeLog;

/// Actions the injected overlay can request. Delivered to the main thread so
/// the HTTP layer never touches UI (design §17-H3).
#[derive(Clone, Copy, Debug)]
pub(crate) enum OverlayAction {
    OpenLogin,
}

/// Startup handshake result passed back to the caller. The bearer token
/// intentionally stays private to this module (in-memory only, design §12);
/// data now reaches the overlay via the CDP push channel instead.
#[derive(Clone, Debug)]
pub(crate) struct OverlayServerInfo {
    pub port: u16,
    // The server caller never exposes this value. It remains private to this
    // module and lets its route tests authenticate without widening IPC.
    #[cfg(test)]
    token: Arc<str>,
}

/// Compact payload for the always-visible bar (design §8).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlayBarPayload {
    auth: &'static str,
    balance: Option<String>,
    today_spend: Option<String>,
    today_tokens: Option<String>,
    fetched_at: Option<String>,
}

/// Full payload for the small window, fetched on demand (design §8).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OverlayDetailPayload {
    auth: &'static str,
    balance: Option<String>,
    today_spend: Option<String>,
    today_tokens: Option<String>,
    today_requests: Option<String>,
    cumulative_tokens: Option<String>,
    input_tokens: Option<String>,
    output_tokens: Option<String>,
    rpm: Option<String>,
    tpm: Option<String>,
    average_response: Option<String>,
    fetched_at: Option<String>,
}

#[derive(Deserialize)]
struct ActionBody {
    action: String,
}

#[derive(Deserialize)]
struct PollQuery {
    rev: Option<u64>,
}

#[derive(Clone)]
struct OverlayCtx {
    token: Arc<str>,
    state: AppState,
    action_tx: mpsc::Sender<OverlayAction>,
    safe_log: SafeLog,
}

const POLL_TIMEOUT_SECS: u64 = 30;

/// Binds a loopback listener on a random port, registers the routes, and
/// returns the chosen port + token. The task runs until the process exits.
pub(crate) async fn spawn(
    state: AppState,
    safe_log: SafeLog,
    action_tx: mpsc::Sender<OverlayAction>,
) -> Result<OverlayServerInfo, std::io::Error> {
    let token: Arc<str> = Arc::from(generate_token());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let ctx = OverlayCtx {
        token: token.clone(),
        state,
        action_tx,
        safe_log: safe_log.clone(),
    };
    safe_log.write_dynamic(
        "overlay_server_started",
        format!("port={port} bind=127.0.0.1"),
    );
    let app = build_router(ctx);
    let serve = axum::serve(listener, app);
    tokio::spawn(async move {
        if let Err(error) = serve.await {
            eprintln!("sevnx overlay server error: {error}");
        }
    });
    Ok(OverlayServerInfo {
        port,
        #[cfg(test)]
        token,
    })
}

fn build_router(ctx: OverlayCtx) -> Router {
    Router::new()
        .route("/sevnx/overlay", get(overlay).options(handle_options))
        .route(
            "/sevnx/overlay/detail",
            get(overlay_detail).options(handle_options),
        )
        .route("/sevnx/overlay/poll", get(overlay_poll).options(handle_options))
        .route("/sevnx/action", post(action).options(handle_options))
        .fallback(handle_fallback)
        .with_state(ctx)
}

fn generate_token() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn auth_label(auth: AuthStatus) -> &'static str {
    match auth {
        AuthStatus::LoggedOut => "logged_out",
        AuthStatus::LoggingIn => "logging_in",
        AuthStatus::Validating => "validating",
        AuthStatus::Authenticated => "authenticated",
        AuthStatus::Expired => "expired",
    }
}

fn bar_payload(snapshot: &AppSnapshot) -> OverlayBarPayload {
    let dashboard = snapshot.dashboard.as_ref();
    OverlayBarPayload {
        auth: auth_label(snapshot.auth),
        balance: dashboard
            .and_then(|d| d.balance.as_ref())
            .and_then(|m| m.display.clone()),
        today_spend: dashboard.and_then(|d| d.today_spend.display_actual.clone()),
        today_tokens: dashboard.and_then(|d| d.today_tokens.display_total.clone()),
        fetched_at: dashboard.map(|d| d.fetched_at.to_rfc3339()),
    }
}

/// Whitelisted snapshot as JSON for the CDP push channel (the Codex page CSP
/// blocks page-initiated loopback fetches, so data is pushed instead). The
/// detail payload is a superset of the bar payload, so one push feeds both.
pub(crate) fn detail_payload_json(snapshot: &AppSnapshot) -> String {
    serde_json::to_string(&detail_payload(snapshot)).unwrap_or_else(|_| "{}".to_string())
}

fn detail_payload(snapshot: &AppSnapshot) -> OverlayDetailPayload {
    let dashboard = snapshot.dashboard.as_ref();
    OverlayDetailPayload {
        auth: auth_label(snapshot.auth),
        balance: dashboard
            .and_then(|d| d.balance.as_ref())
            .and_then(|m| m.display.clone()),
        today_spend: dashboard.and_then(|d| d.today_spend.display_actual.clone()),
        today_tokens: dashboard.and_then(|d| d.today_tokens.display_total.clone()),
        today_requests: dashboard.and_then(|d| d.today_requests.display_total.clone()),
        cumulative_tokens: dashboard.and_then(|d| d.cumulative_tokens.display_total.clone()),
        input_tokens: dashboard.and_then(|d| d.today_tokens.display_input.clone()),
        output_tokens: dashboard.and_then(|d| d.today_tokens.display_output.clone()),
        rpm: dashboard.and_then(|d| d.performance.display_rpm.clone()),
        tpm: dashboard.and_then(|d| d.performance.display_tpm.clone()),
        average_response: dashboard
            .and_then(|d| d.average_response.as_ref())
            .and_then(|v| v.display.clone()),
        fetched_at: dashboard.map(|d| d.fetched_at.to_rfc3339()),
    }
}

fn check_token(ctx: &OverlayCtx, headers: &HeaderMap) -> bool {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .is_some_and(|token| token == ctx.token.as_ref())
}

fn header_value(value: &str) -> axum::http::HeaderValue {
    axum::http::HeaderValue::from_str(value).expect("static header value")
}

fn cors_headers(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        header_value("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        header_value("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        header_value("Content-Type, Authorization"),
    );
    headers.insert(
        header::ACCESS_CONTROL_EXPOSE_HEADERS,
        header_value("X-Overlay-Rev"),
    );
}

fn unauthorized() -> Response {
    let mut response = (StatusCode::UNAUTHORIZED, "invalid token").into_response();
    cors_headers(&mut response);
    response
}

fn bad_request(detail: &'static str) -> Response {
    let mut response = (StatusCode::BAD_REQUEST, detail).into_response();
    cors_headers(&mut response);
    response
}

fn ok_json() -> Response {
    let mut response = Json(serde_json::json!({})).into_response();
    cors_headers(&mut response);
    response
}

async fn handle_options() -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NO_CONTENT;
    cors_headers(&mut response);
    response
}

async fn handle_fallback(State(ctx): State<OverlayCtx>) -> Response {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::NOT_FOUND;
    cors_headers(&mut response);
    ctx.safe_log.write("overlay_unknown_path", "result=404");
    response
}

async fn overlay(State(ctx): State<OverlayCtx>, headers: HeaderMap) -> Response {
    if !check_token(&ctx, &headers) {
        ctx.safe_log.write("overlay_unauthorized", "endpoint=overlay");
        return unauthorized();
    }
    let payload = bar_payload(&ctx.state.snapshot());
    ctx.safe_log.write("overlay_served", "result=ok");
    let mut response = Json(payload).into_response();
    cors_headers(&mut response);
    response
}

async fn overlay_detail(State(ctx): State<OverlayCtx>, headers: HeaderMap) -> Response {
    if !check_token(&ctx, &headers) {
        ctx.safe_log.write("overlay_unauthorized", "endpoint=detail");
        return unauthorized();
    }
    let payload = detail_payload(&ctx.state.snapshot());
    ctx.safe_log.write("overlay_detail_served", "result=ok");
    let mut response = Json(payload).into_response();
    cors_headers(&mut response);
    response
}

async fn overlay_poll(
    State(ctx): State<OverlayCtx>,
    headers: HeaderMap,
    Query(query): Query<PollQuery>,
) -> Response {
    if !check_token(&ctx, &headers) {
        ctx.safe_log.write("overlay_unauthorized", "endpoint=poll");
        return unauthorized();
    }
    let requested = query.rev.unwrap_or(0);
    // `Notified` is `!Unpin`; pin it so it can be awaited/selected repeatedly.
    let mut notified = Box::pin(ctx.state.notify().notified());
    let current = loop {
        let current = ctx.state.rev();
        if current != requested {
            break current;
        }
        tokio::select! {
            _ = &mut notified => {
                notified = Box::pin(ctx.state.notify().notified());
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(POLL_TIMEOUT_SECS)) => {
                break current;
            }
        }
    };
    let payload = bar_payload(&ctx.state.snapshot());
    let mut response = Json(payload).into_response();
    if let Ok(value) = current.to_string().parse() {
        response.headers_mut().insert("X-Overlay-Rev", value);
    }
    cors_headers(&mut response);
    response
}

async fn action(
    State(ctx): State<OverlayCtx>,
    headers: HeaderMap,
    body: Json<ActionBody>,
) -> Response {
    if !check_token(&ctx, &headers) {
        ctx.safe_log.write("overlay_unauthorized", "endpoint=action");
        return unauthorized();
    }
    match body.action.as_str() {
        "open-login" => {
            let _ = ctx.action_tx.send(OverlayAction::OpenLogin).await;
            ctx.safe_log.write("overlay_action", "action=open_login");
            ok_json()
        }
        _ => {
            ctx.safe_log
                .write("overlay_action_rejected", "reason=unknown_action");
            bad_request("unknown action")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{bar_payload, detail_payload};
    use crate::model::{
        ApiKeySummary, AppState, AuthStatus, DashboardSnapshot, PerformanceSummary,
        RequestSummary, SpendSummary, TokenSummary, UsageRange, UsageSnapshot, UsageSummary,
    };

    fn logged_in_state() -> AppState {
        let state = AppState::default();
        state.set_auth_status(AuthStatus::Authenticated);
        let dashboard = DashboardSnapshot {
            balance: None,
            api_keys: ApiKeySummary {
                total: None,
                active: None,
                display_total: Some("2".into()),
                display_active: Some("1".into()),
            },
            today_requests: RequestSummary {
                total: None,
                display_total: Some("42".into()),
            },
            today_spend: SpendSummary {
                actual: None,
                standard: None,
                display_actual: Some("1.23".into()),
                display_standard: None,
            },
            today_tokens: TokenSummary {
                total: None,
                input: None,
                output: None,
                display_total: Some("1000".into()),
                display_input: Some("400".into()),
                display_output: Some("600".into()),
            },
            cumulative_tokens: TokenSummary {
                total: None,
                input: None,
                output: None,
                display_total: Some("5000".into()),
                display_input: None,
                display_output: None,
            },
            performance: PerformanceSummary {
                rpm: None,
                tpm: None,
                display_rpm: Some("10".into()),
                display_tpm: Some("20".into()),
            },
            average_response: None,
            fetched_at: chrono::Utc::now(),
        };
        let usage = UsageSnapshot {
            range: UsageRange::Last24Hours,
            summary: UsageSummary::default(),
            token_trend: Vec::new(),
            models: Vec::new(),
            groups: Vec::new(),
            endpoints: Vec::new(),
            model_pie: Vec::new(),
            fetched_at: chrono::Utc::now(),
        };
        state.replace_success(dashboard, usage, chrono::Utc::now());
        state
    }

    #[test]
    fn bar_payload_serializes_only_whitelisted_fields() {
        let snapshot = logged_in_state().snapshot();
        let payload = bar_payload(&snapshot);
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"auth\":\"authenticated\""));
        assert!(json.contains("\"todaySpend\":\"1.23\""));
        // api_keys data must never leak into the overlay payload.
        assert!(!json.contains("apiKeys"));
        assert!(!json.contains("\"active\":\"1\""));
    }

    #[test]
    fn detail_payload_serializes_whitelisted_extra_fields_but_hides_secrets() {
        let snapshot = logged_in_state().snapshot();
        let payload = detail_payload(&snapshot);
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"todayRequests\":\"42\""));
        assert!(json.contains("\"inputTokens\":\"400\""));
        assert!(json.contains("\"rpm\":\"10\""));
        // api_keys and their display counts must stay out of the detail payload.
        assert!(!json.contains("apiKeys"));
        assert!(!json.contains("\"active\":\"1\""));
    }

    #[test]
    fn detail_payload_reports_an_expired_session() {
        let state = logged_in_state();
        state.mark_auth_expired(crate::api::error::PublicError::new(
            crate::api::error::PublicErrorCode::AuthenticationRequired,
            "登录状态已失效",
        ));

        let payload = detail_payload(&state.snapshot());
        assert_eq!(payload.auth, "expired");
    }

    #[tokio::test]
    async fn overlay_http_endpoints_require_token_and_whitelist_payload() {
        use std::time::{SystemTime, UNIX_EPOCH};

        use crate::services::logging::SafeLog;

        use super::spawn;

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("sevnx-overlay-test-{stamp}"));
        let safe_log = SafeLog::new(dir);
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let info = spawn(logged_in_state(), safe_log, tx).await.unwrap();
        let base = format!("http://127.0.0.1:{}/sevnx", info.port);
        let client = reqwest::Client::new();

        // No token -> 401.
        let resp = client.get(format!("{base}/overlay")).send().await.unwrap();
        assert_eq!(resp.status(), 401);

        // Token -> whitelisted bar payload, never apiKeys.
        let resp = client
            .get(format!("{base}/overlay"))
            .bearer_auth(&info.token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("\"auth\":\"authenticated\""));
        assert!(text.contains("\"todaySpend\":\"1.23\""));
        assert!(!text.contains("apiKeys"));

        // Detail endpoint adds extra fields but still hides apiKeys.
        let resp = client
            .get(format!("{base}/overlay/detail"))
            .bearer_auth(&info.token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let text = resp.text().await.unwrap();
        assert!(text.contains("\"todayRequests\":\"42\""));
        assert!(!text.contains("apiKeys"));

        // Poll with an old rev resolves immediately to the current payload + rev header.
        let resp = client
            .get(format!("{base}/overlay/poll?rev=0"))
            .bearer_auth(&info.token)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let rev = resp
            .headers()
            .get("X-Overlay-Rev")
            .and_then(|value| value.to_str().ok());
        assert!(rev.is_some());
    }
}
