//! Minimal CDP (Chrome DevTools Protocol) client used to inject the SevnX
//! overlay into a Codex page launched with `--remote-debugging-port`.
//!
//! Robustness is modelled on Codex++'s `cdp.rs`:
//! - probes both IPv4 and IPv6 loopback, bypassing any proxy
//! - validates every WebSocket URL is loopback + matches the debug port
//! - selects the *primary* Codex page (excluding avatar-overlay / quick-chat)
//! - `endpoint_available` confirms a real Codex CDP endpoint, not any HTTP port

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{Value, json};
use tauri::Manager;
use tokio_tungstenite::{connect_async, tungstenite::Message};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const HTTP_TIMEOUT: Duration = Duration::from_secs(3);
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);
const PROBE_MAX_BYTES: usize = 256 * 1024;
static OVERLAY_WATCHDOG_STARTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug, Deserialize)]
pub struct CdpTarget {
    pub id: String,
    #[serde(rename = "type")]
    pub target_type: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default, rename = "webSocketDebuggerUrl")]
    pub web_socket_debugger_url: Option<String>,
}

/// Loopback URLs queried for the target list (IPv4 then IPv6).
fn target_urls(debug_port: u16) -> Vec<String> {
    vec![
        format!("http://127.0.0.1:{debug_port}/json"),
        format!("http://[::1]:{debug_port}/json"),
    ]
}

/// Validates a CDP WebSocket URL is ws/wss, loopback, and on the debug port.
fn validate_cdp_websocket_url(url: &str, expected_port: u16) -> Result<(), String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("invalid CDP WebSocket URL: {e}"))?;
    if !matches!(parsed.scheme(), "ws" | "wss") {
        return Err("CDP WebSocket URL must use ws or wss".into());
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| "CDP WebSocket URL has no host".to_string())?;
    let address: IpAddr = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse()
        .map_err(|_| "CDP WebSocket host must be a loopback IP address".to_string())?;
    if !address.is_loopback() {
        return Err("CDP WebSocket host must be loopback".into());
    }
    let port = parsed
        .port()
        .ok_or_else(|| "CDP WebSocket URL must include an explicit port".to_string())?;
    if port != expected_port {
        return Err(format!("CDP WebSocket port {port} does not match debug port {expected_port}"));
    }
    Ok(())
}

/// Lists targets exposed by the Codex debug endpoint (IPv4 + IPv6 loopback).
pub async fn list_targets(debug_port: u16) -> Result<Vec<CdpTarget>, String> {
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(HTTP_TIMEOUT)
        .build()
        .map_err(|e| e.to_string())?;
    let mut errors = Vec::new();
    for url in target_urls(debug_port) {
        match query_targets_url(&client, &url, debug_port).await {
            Ok(targets) => return Ok(targets),
            Err(error) => errors.push(format!("{url}: {error}")),
        }
    }
    Err(format!("failed to query CDP targets on loopback: {}", errors.join("; ")))
}

async fn query_targets_url(
    client: &reqwest::Client,
    url: &str,
    debug_port: u16,
) -> Result<Vec<CdpTarget>, String> {
    let targets = client
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<Vec<CdpTarget>>()
        .await
        .map_err(|e| e.to_string())?;
    for target in &targets {
        if let Some(websocket_url) = target.web_socket_debugger_url.as_deref() {
            validate_cdp_websocket_url(websocket_url, debug_port).map_err(|error| {
                format!("unsafe CDP target WebSocket URL for target {}: {error}", target.id)
            })?;
        }
    }
    Ok(targets)
}

/// True if the loopback debug port exposes a *real Codex* CDP target list.
pub fn endpoint_available(debug_port: u16) -> bool {
    [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), debug_port),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), debug_port),
    ]
    .into_iter()
    .any(|address| probe_endpoint(address, debug_port))
}

fn probe_endpoint(address: SocketAddr, debug_port: u16) -> bool {
    use std::io::{Read, Write};
    let Ok(mut stream) = TcpStream::connect_timeout(&address, PROBE_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(PROBE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(PROBE_TIMEOUT));
    let request =
        format!("GET /json HTTP/1.1\r\nHost: 127.0.0.1:{debug_port}\r\nConnection: close\r\n\r\n");
    if stream.write_all(request.as_bytes()).is_err() {
        return false;
    }
    let mut response = Vec::new();
    let mut chunk = [0_u8; 8192];
    while response.len() < PROBE_MAX_BYTES {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => response.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                break;
            }
            Err(_) => return false,
        }
    }
    response_contains_codex_target(&response, debug_port)
}

fn response_contains_codex_target(response: &[u8], debug_port: u16) -> bool {
    let Some(header_end) = response.windows(4).position(|window| window == b"\r\n\r\n") else {
        return false;
    };
    let headers = String::from_utf8_lossy(&response[..header_end]);
    let status_ok = headers
        .lines()
        .next()
        .is_some_and(|line| line.starts_with("HTTP/") && line.contains(" 200 "));
    if !status_ok {
        return false;
    }
    let Ok(targets) = serde_json::from_slice::<Vec<CdpTarget>>(&response[header_end + 4..]) else {
        return false;
    };
    targets.iter().any(|target| {
        is_codex_endpoint_target(target)
            && target
                .web_socket_debugger_url
                .as_deref()
                .is_some_and(|url| validate_cdp_websocket_url(url, debug_port).is_ok())
    })
}

fn is_injectable_page_target(target: &CdpTarget) -> bool {
    target.target_type == "page"
        && target
            .web_socket_debugger_url
            .as_deref()
            .is_some_and(|url| !url.is_empty())
}

fn is_codex_page_target(target: &CdpTarget) -> bool {
    if target.target_type != "page" {
        return false;
    }
    let haystack = format!("{} {}", target.title, target.url).to_lowercase();
    haystack.contains("codex") || is_chatgpt_desktop_page(&target.title, &target.url)
}

/// The primary Codex window: a Codex page that isn't an avatar overlay or a
/// quick-chat hot-start page.
fn is_primary_codex_page_target(target: &CdpTarget) -> bool {
    is_codex_page_target(target)
        && !is_avatar_overlay_page_target(target)
        && !is_quick_chat_page_target(target)
}

/// A desktop-app page: `app://-/index.html`, with or without an `initialRoute`
/// query. The main window, the avatar overlay and the quick-chat hot-start page
/// all use this URL, so the query decides which one it is — not the title.
fn is_codex_app_page_target(target: &CdpTarget) -> bool {
    let Ok(url) = reqwest::Url::parse(target.url.trim()) else {
        return false;
    };
    url.scheme().eq_ignore_ascii_case("app")
        && url.host_str() == Some("-")
        && url.path().eq_ignore_ascii_case("/index.html")
}

/// The main window: `app://-/index.html` with no `initialRoute` query at all.
///
/// Recent Codex builds expose exactly this single target, titled `ChatGPT`, so
/// nothing in its title or URL contains the string "codex".
fn is_exact_codex_app_main_target(target: &CdpTarget) -> bool {
    target.url.trim().eq_ignore_ascii_case("app://-/index.html")
}

fn is_primary_codex_app_target(target: &CdpTarget) -> bool {
    is_codex_app_page_target(target) && is_primary_codex_page_target(target)
}

fn is_chatgpt_desktop_page_target(target: &CdpTarget) -> bool {
    is_primary_codex_page_target(target) && is_chatgpt_desktop_page(&target.title, &target.url)
}

fn is_supported_codex_page_target(target: &CdpTarget) -> bool {
    is_primary_codex_page_target(target)
        && (is_codex_app_page_target(target) || is_chatgpt_desktop_page(&target.title, &target.url))
}

/// A target the overlay can actually be injected into, in the same order
/// [`pick_codex_page_target`] prefers them.
fn is_codex_endpoint_target(target: &CdpTarget) -> bool {
    is_injectable_page_target(target)
        && (is_exact_codex_app_main_target(target)
            || is_primary_codex_app_target(target)
            || is_chatgpt_desktop_page_target(target))
}

fn is_avatar_overlay_page_target(target: &CdpTarget) -> bool {
    initial_route(target).is_some_and(|route| route.eq_ignore_ascii_case("/avatar-overlay"))
}

fn is_quick_chat_page_target(target: &CdpTarget) -> bool {
    initial_route(target).is_some_and(|route| {
        let route = route.to_ascii_lowercase();
        route == "/chatgpt/quick-chat"
            || route == "/chatgpt/quick-chat-prewarm"
            || route.starts_with("/chatgpt/quick-chat/")
    })
}

fn initial_route(target: &CdpTarget) -> Option<String> {
    if !is_injectable_page_target(target) || !is_codex_app_page_target(target) {
        return None;
    }
    let url = reqwest::Url::parse(target.url.trim()).ok()?;
    url.query_pairs()
        .find(|(key, _)| key.eq_ignore_ascii_case("initialRoute"))
        .map(|(_, value)| value.into_owned())
}

fn is_chatgpt_desktop_page(title: &str, url: &str) -> bool {
    let title = title.trim().to_ascii_lowercase();
    let url = url.trim().to_ascii_lowercase();
    title == "chatgpt"
        && (url == "https://chatgpt.com"
            || url.starts_with("https://chatgpt.com/")
            || url == "https://chat.openai.com"
            || url.starts_with("https://chat.openai.com/")
            || url.starts_with("data:text/html"))
}

/// Picks the page to inject into, most specific first.
///
/// Order matters. The desktop host exposes its main window at
/// `app://-/index.html` titled `ChatGPT`, so matching on the title or URL text
/// finds nothing at all. Auxiliary pages (avatar overlay, quick-chat) share that
/// same URL and must never be preferred over the main window.
fn pick_codex_page_target(targets: &[CdpTarget]) -> Option<CdpTarget> {
    let priorities: [fn(&CdpTarget) -> bool; 4] = [
        is_exact_codex_app_main_target,
        is_primary_codex_app_target,
        is_chatgpt_desktop_page_target,
        is_supported_codex_page_target,
    ];
    for matches_priority in priorities {
        if let Some(target) = targets
            .iter()
            .find(|target| is_injectable_page_target(target) && matches_priority(target))
        {
            return Some(target.clone());
        }
    }
    None
}

/// Evaluates a script in the target renderer and returns the CDP response.
async fn evaluate(websocket_url: &str, script: &str) -> Result<Value, String> {
    let (mut websocket, _) = tokio::time::timeout(CONNECT_TIMEOUT, connect_async(websocket_url))
        .await
        .map_err(|_| "cdp connect timeout".to_string())?
        .map_err(|error| error.to_string())?;

    let id: u64 = 1;
    let command = json!({
        "id": id,
        "method": "Runtime.evaluate",
        "params": {
            "expression": script,
            // Scripts here are synchronous (a boolean health probe or a
            // fire-and-forget IIFE). Awaiting a promise forces the evaluator
            // down a path that can return an object without a usable `value`.
            "awaitPromise": false,
            "returnByValue": true
        }
    });
    websocket
        .send(Message::Text(command.to_string().into()))
        .await
        .map_err(|error| error.to_string())?;

    let result = tokio::time::timeout(COMMAND_TIMEOUT, async {
        loop {
            match websocket.next().await {
                Some(Ok(Message::Text(text))) => {
                    let value: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
                    if value.get("id").and_then(Value::as_u64) == Some(id) {
                        return Ok(value);
                    }
                }
                Some(Ok(_)) => continue,
                Some(Err(error)) => return Err(error.to_string()),
                None => return Err("cdp websocket closed".to_string()),
            }
        }
    })
    .await;
    result.map_err(|_| "cdp command timeout".to_string())?
}

/// Extracts a compact renderer exception description from a CDP response.
/// The message comes from the injected script runtime, not page data; cap it
/// before it reaches the diagnostic log.
fn runtime_exception(response: &Value, operation: &str) -> Option<String> {
    let details = response.pointer("/result/exceptionDetails")?;
    let detail = details
        .pointer("/exception/description")
        .and_then(Value::as_str)
        .or_else(|| details.get("text").and_then(Value::as_str))
        .unwrap_or("unknown renderer exception");
    let detail = detail.chars().take(240).collect::<String>();
    Some(format!("{operation}: {detail}"))
}

/// Picks the primary page target and returns its WebSocket URL.
async fn primary_page_websocket(debug_port: u16) -> Result<String, String> {
    let targets = list_targets(debug_port).await?;
    let target = pick_codex_page_target(&targets)
        .ok_or_else(|| "no injectable Codex page target".to_string())?;
    target
        .web_socket_debugger_url
        .ok_or_else(|| "selected target has no WebSocket URL".to_string())
}

/// Injects the overlay UI script into the Codex primary page. The script is a
/// pure view: data arrives separately via [`push_overlay_data`], so nothing
/// secret is baked in and the page's CSP never sees a foreign origin.
pub async fn inject_overlay(debug_port: u16) -> Result<(), String> {
    let websocket_url = primary_page_websocket(debug_port).await?;
    let response = evaluate(&websocket_url, include_str!("../../resources/overlay.js")).await?;
    // The script may have thrown at runtime; surface that instead of claiming a
    // successful injection so the watchdog keeps retrying.
    if let Some(error) = runtime_exception(&response, "injection script threw") {
        return Err(error);
    }
    Ok(())
}

/// Pushes the whitelisted snapshot JSON into the already-injected overlay via
/// CDP. The renderer-origin CSP blocks page-initiated loopback fetches, but
/// CDP evaluation runs outside that policy, so data travels over the trusted
/// loopback WebSocket instead.
pub async fn push_overlay_data(debug_port: u16, payload_json: &str) -> Result<(), String> {
    let websocket_url = primary_page_websocket(debug_port).await?;
    let script = format!("window.__sevnxOverlayPush?.({payload_json})");
    let response = evaluate(&websocket_url, &script).await?;
    if let Some(error) = runtime_exception(&response, "overlay data push threw") {
        return Err(error);
    }
    Ok(())
}

/// Returns true if the overlay marker is present in the Codex renderer.
/// Used by the re-injection watchdog after Codex reloads / route changes.
pub async fn is_injected(debug_port: u16) -> Result<bool, String> {
    let websocket_url = primary_page_websocket(debug_port).await?;
    let result = evaluate(
        &websocket_url,
        "typeof window.__sevnxOverlayInstalled !== 'undefined' && window.__sevnxOverlayInstalled === true",
    )
    .await?;
    Ok(result
        .pointer("/result/result/value")
        .and_then(Value::as_bool)
        .unwrap_or(false))
}

/// Spawns a background task that keeps the overlay injected across Codex
/// reloads / route changes (design §13). Only logs transitions (injected →
/// lost → recovered) so a closed Codex doesn't spam the log; the initial
/// not-yet-ready retry is silent.
pub fn spawn_overlay_watchdog(app: tauri::AppHandle, debug_port: u16) {
    if OVERLAY_WATCHDOG_STARTED.swap(true, Ordering::AcqRel) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut was_injected = false;
        loop {
            let services = app.state::<crate::app::AppServices>();
            let injected = is_injected(debug_port).await.unwrap_or(false);
            if injected {
                was_injected = true;
            } else if was_injected {
                services
                    .logger()
                    .write_critical("overlay_inject_lost", "reason=renderer_reloaded");
                let recovered = services.inject_codex_overlay(debug_port).await.is_ok();
                services.logger().write(
                    if recovered {
                        "overlay_inject_recovered"
                    } else {
                        "overlay_inject_retry_failed"
                    },
                    if recovered {
                        "result=recovered"
                    } else {
                        "result=failed"
                    },
                );
                was_injected = recovered;
            } else {
                // Initial injection not ready yet; retry silently.
                was_injected = services.inject_codex_overlay(debug_port).await.is_ok();
            }
            if was_injected {
                let payload = services.overlay_push_json();
                if let Err(error) = push_overlay_data(debug_port, &payload).await {
                    services
                        .logger()
                        .write_dynamic("overlay_push_failed", format!("error={error}"));
                }
            }
            tokio::select! {
                _ = services.wait_for_overlay_update() => {}
                _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{CdpTarget, pick_codex_page_target};

    fn target(title: &str, url: &str) -> CdpTarget {
        CdpTarget {
            id: title.to_string(),
            target_type: "page".to_string(),
            title: title.to_string(),
            url: url.to_string(),
            web_socket_debugger_url: Some("ws://127.0.0.1:9229/devtools/page/1".to_string()),
        }
    }

    #[test]
    fn primary_target_selection_skips_auxiliary_pages() {
        let avatar = target(
            "ChatGPT Avatar Overlay",
            "app://-/index.html?initialRoute=%2Favatar-overlay",
        );
        let quick_chat = target(
            "ChatGPT",
            "app://-/index.html?initialRoute=%2Fchatgpt%2Fquick-chat",
        );
        let main = target("ChatGPT", "https://chatgpt.com/");

        let selected = pick_codex_page_target(&[avatar, quick_chat, main]).unwrap();
        assert_eq!(selected.url, "https://chatgpt.com/");
    }

    #[test]
    fn primary_target_selection_does_not_fall_back_to_auxiliary_page() {
        let avatar = target(
            "ChatGPT Avatar Overlay",
            "app://-/index.html?initialRoute=%2Favatar-overlay",
        );
        assert!(pick_codex_page_target(&[avatar]).is_none());
    }

    #[test]
    fn selects_desktop_host_main_page() {
        // Observed on a real Codex 26.903 install: the only target is the main
        // window, titled "ChatGPT" with no "codex" in its title or URL.
        let main = target("ChatGPT", "app://-/index.html");
        let selected = pick_codex_page_target(&[main]).unwrap();
        assert_eq!(selected.url, "app://-/index.html");
    }

    #[test]
    fn desktop_host_main_page_wins_over_auxiliary_pages() {
        let avatar = target(
            "ChatGPT Avatar Overlay",
            "app://-/index.html?initialRoute=%2Favatar-overlay",
        );
        let quick_chat = target(
            "ChatGPT",
            "app://-/index.html?initialRoute=%2Fchatgpt%2Fquick-chat",
        );
        let main = target("ChatGPT", "app://-/index.html");

        let selected = pick_codex_page_target(&[avatar, quick_chat, main]).unwrap();
        assert_eq!(selected.url, "app://-/index.html");
    }

    #[test]
    fn endpoint_accepts_desktop_host_main_page() {
        assert!(super::is_codex_endpoint_target(&target(
            "ChatGPT",
            "app://-/index.html"
        )));
        // An auxiliary page alone is not a usable endpoint.
        assert!(!super::is_codex_endpoint_target(&target(
            "ChatGPT",
            "app://-/index.html?initialRoute=%2Fchatgpt%2Fquick-chat"
        )));
    }
}
