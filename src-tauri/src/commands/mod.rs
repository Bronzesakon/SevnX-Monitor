use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_autostart::ManagerExt as AutoStartExt;
use tauri_plugin_opener::OpenerExt;

use crate::{
    api::error::PublicError,
    app::AppServices,
    auth::webview_login,
    model::{AppSettings, AppSnapshot, AuthStatus, RefreshStatus, UsageRange},
    services::{balance_alert, windows},
};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RefreshStatusEvent {
    refresh: RefreshStatus,
    last_success_at: Option<chrono::DateTime<chrono::Utc>>,
    last_error: Option<crate::api::error::PublicError>,
}

/// Returns only the whitelisted, in-process snapshot. All absent fields are
/// serialized as `null`; the frontend renders those as `--`.
#[tauri::command]
pub fn get_app_snapshot(services: State<'_, AppServices>) -> AppSnapshot {
    services.snapshot()
}

/// Manual refresh is always complete: dashboard, the selected range's usage
/// aggregates and the bar all receive the resulting replacement snapshot.
#[tauri::command]
pub async fn refresh_all(
    app: AppHandle,
    services: State<'_, AppServices>,
) -> Result<AppSnapshot, PublicError> {
    let result = services.refresh_current().await;
    if result.is_ok() {
        balance_alert::evaluate(&app, &services).await;
    }
    emit_app_state(&app, &services);
    result
}

/// 调试触发：无条件使用 refresh_token 刷新会话，结果写入日志供分析。
/// 成功即持久化新的 access_token / refresh_token。
#[tauri::command]
pub async fn test_refresh(
    app: AppHandle,
    services: State<'_, AppServices>,
) -> Result<(), PublicError> {
    let result = services.refresh_session_now().await;
    emit_app_state(&app, &services);
    result
}

#[tauri::command]
pub async fn set_usage_range(
    range: UsageRange,
    app: AppHandle,
    services: State<'_, AppServices>,
) -> Result<AppSnapshot, PublicError> {
    services.set_usage_range(range).await;
    let result = services.refresh_current().await;
    if result.is_ok() {
        balance_alert::evaluate(&app, &services).await;
    }
    emit_app_state(&app, &services);
    result
}

/// Opens the application-owned WebView2 login window. Cookie extraction and
/// validation stay in Rust; this command has no credential parameters.
#[tauri::command]
pub async fn open_login_window(
    app: AppHandle,
    services: State<'_, AppServices>,
) -> Result<(), PublicError> {
    let attempt = services.begin_login().await;
    let access_url = services.settings().await.access_url;
    emit_app_state(&app, &services);
    let result = webview_login::open_login_window(app.clone(), attempt, access_url).await;
    if let Err(error) = &result {
        services.record_login_window_failure(attempt, error).await;
        emit_app_state(&app, &services);
    }
    result
}

#[tauri::command]
pub async fn open_dashboard_in_browser(
    app: AppHandle,
    services: State<'_, AppServices>,
) -> Result<(), PublicError> {
    let access_url = services.settings().await.access_url;
    let dashboard_url = format!("{}/dashboard", access_url.trim_end_matches('/'));
    app.opener()
        .open_url(dashboard_url, None::<&str>)
        .map_err(|_| {
            PublicError::new(
                crate::api::error::PublicErrorCode::Internal,
                "无法打开默认浏览器",
            )
        })
}

#[tauri::command]
pub async fn get_settings(services: State<'_, AppServices>) -> Result<AppSettings, PublicError> {
    Ok(services.settings().await)
}

#[tauri::command]
pub async fn update_settings(
    settings: AppSettings,
    app: AppHandle,
    services: State<'_, AppServices>,
) -> Result<AppSettings, PublicError> {
    settings.validate().map_err(|message| {
        PublicError::new(crate::api::error::PublicErrorCode::InvalidResponse, message)
    })?;
    let previous = services.settings().await;
    if previous.autostart != settings.autostart {
        let result = if settings.autostart {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        };
        result.map_err(|_| {
            PublicError::new(
                crate::api::error::PublicErrorCode::Internal,
                "无法更新开机自动启动设置",
            )
        })?;
    }
    let saved = services.update_settings(settings).await?;
    let _ = app.emit("settings-changed", saved.clone());
    Ok(saved)
}

#[tauri::command]
pub fn show_main_window(app: AppHandle) -> Result<(), PublicError> {
    windows::show_main_window(&app)
}

#[tauri::command]
pub fn hide_main_window(app: AppHandle) -> Result<(), PublicError> {
    windows::hide_main_window(&app)
}

#[tauri::command]
pub fn set_main_window_height(height: f64, app: AppHandle) -> Result<(), PublicError> {
    windows::set_main_window_height(&app, height)
}

#[tauri::command]
pub async fn set_bar_visible(
    visible: bool,
    app: AppHandle,
    services: State<'_, AppServices>,
) -> Result<(), PublicError> {
    windows::set_bar_visibility(&app, visible)?;
    let _ = app.emit("bar-visibility-changed", visible);
    let settings = services.settings().await;
    let _ = app.emit("settings-changed", settings);
    Ok(())
}

/// 用系统默认方式打开最新一次运行的日志文件，便于排查续期/刷新问题。
#[tauri::command]
pub fn open_log_file(app: AppHandle, services: State<'_, AppServices>) -> Result<(), PublicError> {
    let log_path = services.paths().logs_dir().join("sevnx-monitor.log");
    app.opener()
        .open_path(log_path.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|_| {
            PublicError::new(
                crate::api::error::PublicErrorCode::Internal,
                "无法打开日志文件",
            )
        })
}

/// 创建指向本程序、带 `sevnx://relaunch?dbg=PORT` 参数的 Codex 快捷方式，
/// 点击即可启动 SevnX 并注入 Codex 横条（设计 §9/§13）。默认创建到桌面，
/// 也可指定启动菜单 / 开始菜单。图标取自已安装的 Codex 原版图标。
#[tauri::command]
pub fn create_codex_shortcut(
    location: Option<crate::services::shortcut::ShortcutLocation>,
) -> Result<String, PublicError> {
    let location = location.unwrap_or(crate::services::shortcut::ShortcutLocation::Desktop);
    crate::services::shortcut::create_codex_shortcut(location)
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|_| {
            PublicError::new(
                crate::api::error::PublicErrorCode::Internal,
                "无法创建快捷方式",
            )
        })
}

/// 拉起带注入状态窗的 Codex 进程：未运行则启动并注入；已运行则只注入或提示。
/// 返回面向用户的提示文案，由前端顶部弹出。
#[tauri::command]
pub async fn launch_codex(app: AppHandle) -> Result<String, PublicError> {
    crate::services::codex_launcher::launch_and_inject(&app).await.map_err(|message| {
        PublicError::new(crate::api::error::PublicErrorCode::Internal, message)
    })
}

/// The real exit operation is intentionally kept in the tray handler. A web
/// view cannot cause the app to terminate by invoking this public command.
#[tauri::command]
pub fn request_exit() -> Result<(), PublicError> {
    Err(PublicError::new(
        crate::api::error::PublicErrorCode::PermissionDenied,
        "请使用托盘菜单中的“退出程序”",
    ))
}

pub fn emit_app_state(app: &AppHandle, services: &AppServices) {
    let snapshot = services.snapshot();
    let _ = app.emit("app-snapshot-changed", snapshot.clone());
    let _ = app.emit(
        "refresh-status-changed",
        RefreshStatusEvent {
            refresh: snapshot.refresh,
            last_success_at: snapshot.last_success_at,
            last_error: snapshot.last_error.clone(),
        },
    );
    let _ = app.emit("auth-status-changed", snapshot.auth);
}

pub fn emit_settings(app: &AppHandle, settings: AppSettings) {
    let _ = app.emit("settings-changed", settings);
}

pub fn emit_navigation_to_settings(app: &AppHandle) {
    let _ = app.emit("navigate-settings", ());
}

pub fn is_authenticated(services: &AppServices) -> bool {
    services.snapshot().auth == AuthStatus::Authenticated
}
