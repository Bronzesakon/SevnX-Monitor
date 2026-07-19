use std::time::{Duration, Instant};

use chrono::Utc;
use tauri::{AppHandle, Manager};

use crate::{app::AppServices, commands, model::AuthStatus, services::balance_alert};

/// A small visibility-aware timer. It polls window visibility without opening
/// any local port and delegates all network work to the refresh coordinator,
/// which coalesces overlapping manual and automatic triggers.
pub fn start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        // Startup has its own restore-and-refresh task in `lib.rs`. Begin the
        // periodic clock from now so that task is not followed immediately by
        // a second full request cycle after the session becomes authenticated.
        let mut last_attempt = Instant::now();
        let mut was_visible = true;
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
            let services = app.state::<AppServices>();
            let settings = services.settings().await;
            if !settings.auto_refresh || services.snapshot().auth != AuthStatus::Authenticated {
                was_visible = visible_windows(&app);
                continue;
            }

            let visible = visible_windows(&app);
            let interval = if visible {
                Duration::from_secs(30)
            } else {
                Duration::from_secs(5 * 60)
            };
            let snapshot = services.snapshot();
            let stale_after_restore = visible
                && !was_visible
                && snapshot
                    .last_success_at
                    .is_none_or(|last| (Utc::now() - last).num_seconds() >= 30);
            if stale_after_restore || last_attempt.elapsed() >= interval {
                last_attempt = Instant::now();
                let result = services.refresh_current().await;
                if result.is_ok() {
                    balance_alert::evaluate(&app, &services).await;
                }
                commands::emit_app_state(&app, &services);
            }
            was_visible = visible;
        }
    });
}

fn visible_windows(app: &AppHandle) -> bool {
    ["main", "bar"].into_iter().any(|label| {
        app.get_webview_window(label)
            .and_then(|window| window.is_visible().ok())
            .unwrap_or(false)
    })
}
