use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, UserAttentionType, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder, WindowEvent,
};

use crate::{
    api::error::{PublicError, PublicErrorCode},
    app::AppServices,
    storage::StoredBarPosition,
};

pub const MAIN_LABEL: &str = "main";
pub const BAR_LABEL: &str = "bar";
const BAR_WIDTH: f64 = 420.0;
const BAR_HEIGHT: f64 = 32.0;
const SNAP_DISTANCE: i32 = 22;
const MAIN_MAX_HEIGHT: f64 = 900.0;

pub fn create_bar_window(
    app: &AppHandle,
    visible: bool,
    stored_position: Option<StoredBarPosition>,
) -> Result<WebviewWindow, PublicError> {
    if let Some(existing) = app.get_webview_window(BAR_LABEL) {
        return Ok(existing);
    }

    let (initial_position, scale_factor) = initial_bar_placement(app, stored_position.as_ref());
    let bar = WebviewWindowBuilder::new(app, BAR_LABEL, WebviewUrl::App("index.html".into()))
        .title("SevnX Monitor")
        .inner_size(BAR_WIDTH, BAR_HEIGHT)
        .min_inner_size(BAR_WIDTH, BAR_HEIGHT)
        .max_inner_size(BAR_WIDTH, BAR_HEIGHT)
        .position(
            initial_position.x as f64 / scale_factor,
            initial_position.y as f64 / scale_factor,
        )
        .resizable(false)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
        .map_err(|_| PublicError::new(PublicErrorCode::Internal, "无法创建吸附横条"))?;
    bar.set_position(initial_position)
        .map_err(|_| PublicError::new(PublicErrorCode::Internal, "无法定位吸附横条"))?;
    if let Ok(position) = bar.outer_position() {
        snap_bar_window(app, &bar, position);
    }
    if visible {
        bar.show()
            .map_err(|_| PublicError::new(PublicErrorCode::Internal, "无法显示吸附横条"))?;
    }
    Ok(bar)
}

pub fn set_bar_visibility(app: &AppHandle, visible: bool) -> Result<(), PublicError> {
    let bar = if let Some(bar) = app.get_webview_window(BAR_LABEL) {
        bar
    } else {
        let services = app.state::<AppServices>();
        let (_, position) = services.initial_bar_state();
        create_bar_window(app, visible, position)?
    };
    if visible {
        bar.show()
            .and_then(|_| bar.set_focus())
            .map_err(|_| PublicError::new(PublicErrorCode::Internal, "无法显示吸附横条"))
    } else {
        bar.hide()
            .map_err(|_| PublicError::new(PublicErrorCode::Internal, "无法隐藏吸附横条"))
    }
}

pub fn show_main_window(app: &AppHandle) -> Result<(), PublicError> {
    let window = match app.get_webview_window(MAIN_LABEL) {
        Some(window) => window,
        None => return Err(main_window_restore_error(app, "stage=lookup")),
    };

    // A hidden-to-tray window can also be minimized. Restore that state before
    // showing it, then bring it to the foreground from the tray interaction.
    if window.set_skip_taskbar(false).is_err() {
        return Err(main_window_restore_error(app, "stage=taskbar"));
    }
    if window.unminimize().is_err() {
        return Err(main_window_restore_error(app, "stage=unminimize"));
    }
    if window.show().is_err() {
        return Err(main_window_restore_error(app, "stage=show"));
    }
    if window.set_focus().is_err() {
        app.state::<AppServices>()
            .record_main_window_restore_failure("stage=focus");
        // Windows can deny foreground activation in a few system-owned focus
        // transitions. The visible window remains usable and the taskbar cue
        // gives the user an immediate route back to it.
        if window
            .request_user_attention(Some(UserAttentionType::Informational))
            .is_err()
        {
            app.state::<AppServices>()
                .record_main_window_restore_failure("stage=attention");
        }
    }
    Ok(())
}

fn main_window_restore_error(app: &AppHandle, stage: &'static str) -> PublicError {
    app.state::<AppServices>()
        .record_main_window_restore_failure(stage);
    PublicError::new(PublicErrorCode::Internal, "无法显示主窗口")
}

pub fn hide_main_window(app: &AppHandle) -> Result<(), PublicError> {
    let window = app
        .get_webview_window(MAIN_LABEL)
        .ok_or_else(|| PublicError::new(PublicErrorCode::Internal, "主窗口不可用"))?;
    window
        .set_skip_taskbar(true)
        .and_then(|_| window.hide())
        .map_err(|_| PublicError::new(PublicErrorCode::Internal, "无法隐藏主窗口"))
}

pub fn set_main_window_height(app: &AppHandle, height: f64) -> Result<(), PublicError> {
    let window = app
        .get_webview_window(MAIN_LABEL)
        .ok_or_else(|| PublicError::new(PublicErrorCode::Internal, "主窗口不可用"))?;
    window
        .set_size(tauri::LogicalSize::new(
            400.0,
            height.clamp(1.0, MAIN_MAX_HEIGHT),
        ))
        .map_err(|_| PublicError::new(PublicErrorCode::Internal, "无法调整主窗口高度"))
}

pub fn handle_window_event(app: &AppHandle, label: &str, event: &WindowEvent) {
    match (label, event) {
        (MAIN_LABEL, WindowEvent::CloseRequested { api, .. }) => {
            api.prevent_close();
            let _ = hide_main_window(app);
        }
        (BAR_LABEL, WindowEvent::Moved(position)) => {
            if let Some(bar) = app.get_webview_window(BAR_LABEL) {
                snap_bar_window(app, &bar, *position);
            }
        }
        (BAR_LABEL, WindowEvent::ScaleFactorChanged { .. }) => {
            if let Some(bar) = app.get_webview_window(BAR_LABEL)
                && let Ok(position) = bar.outer_position()
            {
                snap_bar_window(app, &bar, position);
            }
        }
        (BAR_LABEL, WindowEvent::CloseRequested { api, .. }) => {
            api.prevent_close();
            let _ = app
                .get_webview_window(BAR_LABEL)
                .and_then(|bar| bar.hide().ok().map(|_| bar));
        }
        _ => {}
    }
}

fn snap_bar_window(app: &AppHandle, bar: &WebviewWindow, position: PhysicalPosition<i32>) {
    let Ok(Some(monitor)) = bar.current_monitor() else {
        return;
    };
    let Ok(size) = bar.outer_size() else {
        return;
    };
    let work = monitor.work_area();
    let left = work.position.x;
    let top = work.position.y;
    let last_position = last_bar_position(work.position, work.size, size);
    let right = last_position.x;
    let bottom = last_position.y;
    let clamped = clamp_bar_position(position, work.position, work.size, size);
    let mut x = clamped.x;
    let mut y = clamped.y;
    let mut edge = None;
    if (x - left).abs() <= SNAP_DISTANCE {
        x = left;
        edge = Some("left");
    } else if (x - right).abs() <= SNAP_DISTANCE {
        x = right;
        edge = Some("right");
    }
    if (y - top).abs() <= SNAP_DISTANCE {
        y = top;
        edge = Some("top");
    } else if (y - bottom).abs() <= SNAP_DISTANCE {
        y = bottom;
        edge = Some("bottom");
    }
    if x != position.x || y != position.y {
        let _ = bar.set_position(PhysicalPosition::new(x, y));
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        app.state::<AppServices>()
            .set_bar_position(StoredBarPosition {
                x,
                y,
                edge: edge.map(str::to_owned),
            })
            .await;
    });
}

fn initial_bar_placement(
    app: &AppHandle,
    stored_position: Option<&StoredBarPosition>,
) -> (PhysicalPosition<i32>, f64) {
    let monitors = app.available_monitors().unwrap_or_default();
    if let Some(stored) = stored_position {
        let position = PhysicalPosition::new(stored.x, stored.y);
        if let Some(monitor) = monitors
            .iter()
            .find(|monitor| point_in_rect(position, *monitor.position(), *monitor.size()))
        {
            let work = monitor.work_area();
            let scale_factor = monitor.scale_factor();
            return (
                clamp_bar_position(
                    position,
                    work.position,
                    work.size,
                    physical_bar_size(scale_factor),
                ),
                scale_factor,
            );
        }
    }

    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| monitors.into_iter().next());
    let Some(monitor) = primary else {
        return (PhysicalPosition::new(20, 0), 1.0);
    };
    let work = monitor.work_area();
    let scale_factor = monitor.scale_factor();
    (
        centered_bar_position(work.position, work.size, physical_bar_size(scale_factor)),
        scale_factor,
    )
}

fn physical_bar_size(scale_factor: f64) -> PhysicalSize<u32> {
    PhysicalSize::new(
        (BAR_WIDTH * scale_factor).round().max(1.0) as u32,
        (BAR_HEIGHT * scale_factor).round().max(1.0) as u32,
    )
}

fn centered_bar_position(
    work_position: PhysicalPosition<i32>,
    work_size: PhysicalSize<u32>,
    bar_size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let available_width = work_size.width.saturating_sub(bar_size.width);
    PhysicalPosition::new(
        work_position
            .x
            .saturating_add(u32_to_i32(available_width / 2)),
        work_position.y,
    )
}

fn clamp_bar_position(
    position: PhysicalPosition<i32>,
    work_position: PhysicalPosition<i32>,
    work_size: PhysicalSize<u32>,
    bar_size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    let last = last_bar_position(work_position, work_size, bar_size);
    PhysicalPosition::new(
        position.x.clamp(work_position.x, last.x),
        position.y.clamp(work_position.y, last.y),
    )
}

fn last_bar_position(
    work_position: PhysicalPosition<i32>,
    work_size: PhysicalSize<u32>,
    bar_size: PhysicalSize<u32>,
) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        work_position
            .x
            .saturating_add(u32_to_i32(work_size.width.saturating_sub(bar_size.width))),
        work_position
            .y
            .saturating_add(u32_to_i32(work_size.height.saturating_sub(bar_size.height))),
    )
}

fn point_in_rect(
    point: PhysicalPosition<i32>,
    rect_position: PhysicalPosition<i32>,
    rect_size: PhysicalSize<u32>,
) -> bool {
    let right = rect_position.x.saturating_add(u32_to_i32(rect_size.width));
    let bottom = rect_position.y.saturating_add(u32_to_i32(rect_size.height));
    point.x >= rect_position.x && point.x < right && point.y >= rect_position.y && point.y < bottom
}

fn u32_to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}

#[cfg(test)]
mod tests {
    use tauri::{PhysicalPosition, PhysicalSize};

    use super::{centered_bar_position, clamp_bar_position, point_in_rect};

    #[test]
    fn clamps_a_restored_position_to_the_work_area() {
        let position = clamp_bar_position(
            PhysicalPosition::new(1800, 1050),
            PhysicalPosition::new(0, 0),
            PhysicalSize::new(1920, 1040),
            PhysicalSize::new(525, 40),
        );

        assert_eq!(position, PhysicalPosition::new(1395, 1000));
    }

    #[test]
    fn preserves_positions_on_a_negative_origin_monitor() {
        let position = clamp_bar_position(
            PhysicalPosition::new(-1800, 80),
            PhysicalPosition::new(-1920, 0),
            PhysicalSize::new(1920, 1080),
            PhysicalSize::new(420, 32),
        );

        assert_eq!(position, PhysicalPosition::new(-1800, 80));
        assert!(point_in_rect(
            position,
            PhysicalPosition::new(-1920, 0),
            PhysicalSize::new(1920, 1080),
        ));
    }

    #[test]
    fn oversized_bar_is_pinned_to_the_work_area_origin() {
        let position = clamp_bar_position(
            PhysicalPosition::new(900, 700),
            PhysicalPosition::new(100, 50),
            PhysicalSize::new(300, 20),
            PhysicalSize::new(420, 32),
        );

        assert_eq!(position, PhysicalPosition::new(100, 50));
    }

    #[test]
    fn centers_the_bar_using_physical_dimensions() {
        let position = centered_bar_position(
            PhysicalPosition::new(1920, 0),
            PhysicalSize::new(2560, 1400),
            PhysicalSize::new(630, 48),
        );

        assert_eq!(position, PhysicalPosition::new(2885, 0));
    }
}
