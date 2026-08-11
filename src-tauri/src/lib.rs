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
    let requested_debug_port = parse_requested_debug_port(&std::env::args().collect::<Vec<_>>());
    let services = app::AppServices::new().expect("failed to initialize SevnX Monitor services");
    tauri::Builder::default()
        // This plugin must be first so a second launch focuses the existing
        // application before any other initialization runs.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            let _ = services::windows::show_main_window(app);
            // A second launch via `sevnx://relaunch?dbg=PORT` relays the port
            // through the same lifecycle as the in-app launch command.
            if let Some(port) = parse_requested_debug_port(&argv) {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let services = app.state::<crate::app::AppServices>();
                    if let Err(error) = services::codex_launcher::relaunch_and_inject(&app, port).await {
                        services.logger().write_dynamic(
                            "protocol_relaunch_failed",
                            format!("port={port} error={error}"),
                        );
                    }
                });
            }
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None::<Vec<&'static str>>,
        ))
        .manage(services)
        .setup(move |app| {
            services::tray::setup(app)?;
            // Explicitly restore and focus the main window on the initial launch.
            services::windows::show_main_window(app.handle())
                .map_err(|error| std::io::Error::other(error.message))?;
            let services = app.state::<app::AppServices>();
            let (bar_visible, bar_position) = services.initial_bar_state();
            services::windows::create_bar_window(app.handle(), bar_visible, bar_position)
                .map_err(|error| std::io::Error::other(error.message))?;

            // Overlay server: injected-Codex clients reach local data/Actions
            // over loopback HTTP. Actions are bridged to this main-thread loop
            // so the HTTP layer never touches UI (design §17-H3).
            let (overlay_action_tx, mut overlay_action_rx) =
                tokio::sync::mpsc::channel::<services::overlay_server::OverlayAction>(16);
            let action_app = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                while let Some(action) = overlay_action_rx.recv().await {
                    let services = action_app.state::<app::AppServices>();
                    match action {
                        services::overlay_server::OverlayAction::OpenLogin => {
                            let attempt = services.begin_login().await;
                            let access_url = services.settings().await.access_url;
                            if let Err(error) = crate::auth::webview_login::open_login_window(
                                action_app.clone(),
                                attempt,
                                access_url,
                            )
                            .await
                            {
                                services.record_login_window_failure(attempt, &error).await;
                            }
                        }
                    }
                }
            });
            let overlay_app = app.handle().clone();
            let startup_debug_port = requested_debug_port;
            tauri::async_runtime::spawn(async move {
                let services = overlay_app.state::<app::AppServices>();
                if let Err(error) = services.start_overlay(overlay_action_tx).await {
                    eprintln!("failed to start overlay server: {error}");
                } else if let Some(port) = startup_debug_port {
                    if let Err(error) = services::codex_launcher::relaunch_and_inject(&overlay_app, port).await {
                        services.logger().write_dynamic(
                            "protocol_relaunch_failed",
                            format!("port={port} error={error}"),
                        );
                    }
                    // Watchdog: keep the overlay alive across Codex reloads. Only
                    // logs a transition (injected → lost → recovered) so a closed
                    // Codex doesn't spam the log every 5s.
                    services::codex_inject::spawn_overlay_watchdog(overlay_app, port);
                }
            });

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
            commands::test_refresh,
            commands::set_usage_range,
            commands::open_login_window,
            commands::open_dashboard_in_browser,
            commands::get_settings,
            commands::update_settings,
            commands::show_main_window,
            commands::hide_main_window,
            commands::set_main_window_height,
            commands::set_bar_visible,
            commands::open_log_file,
            commands::create_codex_shortcut,
            commands::launch_codex,
            commands::request_exit,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run SevnX Monitor");
}

/// Extracts the requested Codex debug port from a `sevnx://relaunch?dbg=PORT`
/// command-line argument (the shell passes the full URL as one argv entry).
fn parse_requested_debug_port(args: &[String]) -> Option<u16> {
    args.iter().find_map(|arg| {
        let (scheme_and_path, query) = arg.split_once('?')?;
        if !scheme_and_path.eq_ignore_ascii_case("sevnx://relaunch") {
            return None;
        }
        query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            key.eq_ignore_ascii_case("dbg")
                .then(|| value.trim().parse::<u16>().ok())
                .flatten()
                .filter(|port| *port != 0)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::parse_requested_debug_port;

    #[test]
    fn extracts_port_from_uri_argument_without_depending_on_argument_order() {
        let args = vec![
            "sevnx-monitor.exe".to_string(),
            "SEVNX://RELAUNCH?source=overlay&DBG=9229".to_string(),
        ];
        assert_eq!(parse_requested_debug_port(&args), Some(9229));
    }

    #[test]
    fn rejects_non_relaunch_and_invalid_ports() {
        assert_eq!(parse_requested_debug_port(&["sevnx://other?dbg=9229".into()]), None);
        assert_eq!(parse_requested_debug_port(&["sevnx://relaunch?dbg=0".into()]), None);
    }
}
