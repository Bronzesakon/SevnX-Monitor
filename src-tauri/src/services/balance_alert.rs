use chrono::Utc;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

use crate::{app::AppServices, storage::StoredAlertState};

/// Evaluates only the frozen low-balance rule after a successful snapshot
/// replacement. Ordinary network errors never enter this path and therefore
/// never produce a Windows notification.
pub async fn evaluate(app: &AppHandle, services: &AppServices) {
    let settings = services.settings().await;
    if !settings.low_balance_alert {
        return;
    }
    let Some(balance) = services
        .snapshot()
        .dashboard
        .and_then(|dashboard| dashboard.balance)
        .and_then(|balance| balance.value)
    else {
        return;
    };

    let mut state = services.alert_state().await;
    if balance > settings.low_balance_threshold {
        if state.was_low || state.last_notified_at.is_some() {
            state = StoredAlertState::default();
            services.set_alert_state(state).await;
        }
        return;
    }

    let now = Utc::now();
    let due = !state.was_low
        || state.last_notified_at.is_none_or(|previous| {
            (now - previous).num_seconds() >= settings.alert_interval.seconds()
        });
    if !due {
        return;
    }

    // Display the server value with the product's fixed currency symbol. The
    // notification has no account identifier, credentials or response body.
    let body = format!("当前余额 ¥{}，低于或等于设定阈值。", balance);
    if app
        .notification()
        .builder()
        .title("SevnX Monitor 低余额提醒")
        .body(body)
        .show()
        .is_ok()
    {
        state.was_low = true;
        state.last_notified_at = Some(now);
        services.set_alert_state(state).await;
    }
}
