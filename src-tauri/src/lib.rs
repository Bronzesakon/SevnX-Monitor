#![allow(linker_messages)]

//! SevnX Monitor's Rust application core.
//!
//! Public IPC models are whitelist-shaped and never contain credentials or raw
//! response bodies. Browser login, DPAPI, requests, scheduling, tray and
//! Windows integration remain on the Rust side of the Tauri boundary.

pub mod api;
pub mod app;
pub mod auth;
pub mod commands;
pub mod model;
pub mod security;
pub mod services;
pub mod storage;

use tauri::Manager;

pub fn run() {
    let services = app::AppServices::new().expect("failed to initialize SevnX Monitor services");
    tauri::Builder::default()
        // This plugin must be first so a second launch focuses the existing
        // application before any other initialization runs.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let _ = services::windows::show_main_window(app);
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&'static str>>,
        ))
        .manage(services)
        .setup(|app| {
            services::tray::setup(app)?;
            // Explicitly restore and focus the main window on the initial launch.
            services::windows::show_main_window(app.handle())
                .map_err(|error| std::io::Error::other(error.message))?;
            let services = app.state::<app::AppServices>();
            let (bar_visible, bar_position) = services.initial_bar_state();
            services::windows::create_bar_window(app.handle(), bar_visible, bar_position)
                .map_err(|error| std::io::Error::other(error.message))?;

            let startup_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let services = startup_app.state::<app::AppServices>();
                let snapshot = services.restore_and_refresh_session().await;
                if snapshot.refresh == model::RefreshStatus::Success {
                    services::balance_alert::evaluate(&startup_app, &services).await;
                }
                commands::emit_app_state(&startup_app, &services);
                commands::emit_settings(&startup_app, services.settings().await);
            });
            services::auto_refresh::start(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            services::windows::handle_window_event(window.app_handle(), window.label(), event);
            auth::webview_login::handle_window_event(
                window.app_handle().clone(),
                window.label(),
                event,
            );
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_snapshot,
            commands::refresh_all,
            commands::set_usage_range,
            commands::open_login_window,
            commands::open_dashboard_in_browser,
            commands::get_settings,
            commands::update_settings,
            commands::show_main_window,
            commands::hide_main_window,
            commands::set_main_window_height,
            commands::set_bar_visible,
            commands::copy_diagnostics,
            commands::request_exit,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SevnX Monitor");
}
