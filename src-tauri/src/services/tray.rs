use tauri::{
    App, AppHandle, Emitter, Manager,
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
};

use crate::{
    app::AppServices,
    commands,
    services::{balance_alert, windows},
};

const OPEN_MAIN: &str = "open-main";
const TOGGLE_BAR: &str = "toggle-bar";
const REFRESH: &str = "refresh";
const SETTINGS: &str = "settings";
const EXIT: &str = "exit";

pub fn setup(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, OPEN_MAIN, "打开主窗口", true, None::<&str>)?;
    let bar = MenuItem::with_id(app, TOGGLE_BAR, "显示/隐藏横条", true, None::<&str>)?;
    let refresh = MenuItem::with_id(app, REFRESH, "刷新", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, SETTINGS, "设置", true, None::<&str>)?;
    let exit = MenuItem::with_id(app, EXIT, "退出程序", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &bar, &refresh, &settings, &exit])?;
    let icon = Image::from_bytes(include_bytes!("../../icons/tray-icon@2x.png"))?;
    let tray = TrayIconBuilder::with_id("sevnx-monitor-tray")
        .icon(icon)
        .tooltip("SevnX Monitor")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                tauri::tray::TrayIconEvent::Click {
                    button: tauri::tray::MouseButton::Left,
                    button_state: tauri::tray::MouseButtonState::Down,
                    ..
                }
            ) {
                restore_main_window_from_tray(tray.app_handle());
            }
        })
        .build(app)?;
    app.manage(tray);
    Ok(())
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    match event.id().as_ref() {
        OPEN_MAIN => {
            restore_main_window_from_tray(app);
        }
        TOGGLE_BAR => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let services = app.state::<AppServices>();
                let mut settings = services.settings().await;
                settings.bar_visible = !settings.bar_visible;
                let visible = settings.bar_visible;
                if services.update_settings(settings.clone()).await.is_ok()
                    && windows::set_bar_visibility(&app, visible).is_ok()
                {
                    let _ = app.emit("settings-changed", settings);
                    let _ = app.emit("bar-visibility-changed", visible);
                }
            });
        }
        REFRESH => {
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let services = app.state::<AppServices>();
                let result = services.refresh_current().await;
                if result.is_ok() {
                    balance_alert::evaluate(&app, &services).await;
                }
                commands::emit_app_state(&app, &services);
            });
        }
        SETTINGS => {
            restore_main_window_from_tray(app);
            commands::emit_navigation_to_settings(app);
        }
        EXIT => app.exit(0),
        _ => {}
    }
}

fn restore_main_window_from_tray(app: &AppHandle) {
    if windows::show_main_window(app).is_err() {
        // `show_main_window` has already logged the precise fixed stage. This
        // source label confirms that the failed attempt came from a tray menu.
        app.state::<AppServices>()
            .record_main_window_restore_failure("source=tray");
    }
}
