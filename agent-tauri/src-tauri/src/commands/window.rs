use std::sync::atomic::{AtomicU64, Ordering};

use tauri::{AppHandle, Manager};

use crate::tray;

static WINDOW_MOTION_GENERATION: AtomicU64 = AtomicU64::new(0);

fn cancel_window_motion() {
    WINDOW_MOTION_GENERATION.fetch_add(1, Ordering::SeqCst);
}

#[tauri::command]
pub(crate) fn hide_main_window(app: AppHandle) {
    cancel_window_motion();
    tray::hide_main_window(&app);
}

#[tauri::command]
pub(crate) fn complete_tray_hide(app: AppHandle) {
    cancel_window_motion();
    // Hide only after the visible surface has fully animated away, then destroy
    // the WebView shortly afterwards. The Agent process and tray stay alive.
    tray::hide_main_window(&app);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        tray::destroy_main_window(&app);
    });
}

#[tauri::command]
pub(crate) fn minimize_main_window(app: AppHandle) -> Result<(), String> {
    cancel_window_motion();
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "메인 창을 찾을 수 없습니다.".to_string())?;
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn request_tray_hide(app: AppHandle) {
    cancel_window_motion();
    tray::request_animated_hide(&app);
}

#[tauri::command]
pub(crate) fn start_main_window_drag(app: AppHandle) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "메인 창을 찾을 수 없습니다.".to_string())?;

    #[cfg(windows)]
    {
        let generation = WINDOW_MOTION_GENERATION
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);
        std::thread::Builder::new()
            .name("yummi-window-physics".into())
            .spawn(move || run_windows_physics_drag(window, generation))
            .map(|_| ())
            .map_err(|error| format!("창 물리 스레드를 시작하지 못했습니다: {error}"))
    }

    #[cfg(not(windows))]
    {
        cancel_window_motion();
        window.start_dragging().map_err(|error| error.to_string())
    }
}

#[cfg(windows)]
fn run_windows_physics_drag(window: tauri::WebviewWindow, generation: u64) {
    use std::{
        collections::VecDeque,
        thread,
        time::{Duration, Instant},
    };

    use tauri::{PhysicalPosition, Position};

    const DRAG_INTERVAL: Duration = Duration::from_millis(8);
    const VELOCITY_SAMPLE_WINDOW: Duration = Duration::from_millis(72);
    const FRICTION_PER_60HZ_FRAME: f64 = 0.965;
    const BOUNCE: f64 = 0.68;
    const STOP_SPEED_PX_S: f64 = 22.0;
    const MIN_THROW_SPEED_PX_S: f64 = 55.0;
    const MAX_THROW_SPEED_PX_S: f64 = 3600.0;

    if !motion_is_current(generation) || !left_button_down() {
        return;
    }

    let Some(initial_cursor) = cursor_position() else {
        return;
    };
    let Ok(initial_position) = window.outer_position() else {
        return;
    };

    let offset_x = initial_position.x - initial_cursor.x;
    let offset_y = initial_position.y - initial_cursor.y;
    let mut samples = VecDeque::with_capacity(16);

    while motion_is_current(generation) && left_button_down() {
        let Some(cursor) = cursor_position() else {
            break;
        };
        let now = Instant::now();
        let x = cursor.x.saturating_add(offset_x);
        let y = cursor.y.saturating_add(offset_y);

        if window
            .set_position(Position::Physical(PhysicalPosition::new(x, y)))
            .is_err()
        {
            return;
        }

        samples.push_back((now, x, y));
        while samples
            .front()
            .map(|(time, _, _)| now.saturating_duration_since(*time) > VELOCITY_SAMPLE_WINDOW)
            .unwrap_or(false)
        {
            samples.pop_front();
        }

        thread::sleep(DRAG_INTERVAL);
    }

    if !motion_is_current(generation) {
        return;
    }

    let Ok(position) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };

    let mut x = position.x as f64;
    let mut y = position.y as f64;
    let width = size.width as f64;
    let height = size.height as f64;

    let (mut vx, mut vy) = release_velocity(&samples);
    let speed = vx.hypot(vy);
    if speed < MIN_THROW_SPEED_PX_S {
        return;
    }
    if speed > MAX_THROW_SPEED_PX_S {
        let scale = MAX_THROW_SPEED_PX_S / speed;
        vx *= scale;
        vy *= scale;
    }

    let mut previous = Instant::now();

    loop {
        if !motion_is_current(generation) {
            return;
        }

        let now = Instant::now();
        let dt = now
            .saturating_duration_since(previous)
            .as_secs_f64()
            .clamp(0.001, 0.032);
        previous = now;

        x += vx * dt;
        y += vy * dt;

        resolve_desktop_collisions(&mut x, &mut y, width, height, &mut vx, &mut vy, BOUNCE);

        if window
            .set_position(Position::Physical(PhysicalPosition::new(
                x.round() as i32,
                y.round() as i32,
            )))
            .is_err()
        {
            return;
        }

        let friction = FRICTION_PER_60HZ_FRAME.powf(dt / (1.0 / 60.0));
        vx *= friction;
        vy *= friction;

        if vx.hypot(vy) < STOP_SPEED_PX_S {
            return;
        }

        let spent = Instant::now().saturating_duration_since(now);
        if spent < DRAG_INTERVAL {
            thread::sleep(DRAG_INTERVAL - spent);
        }
    }
}

#[cfg(windows)]
fn motion_is_current(generation: u64) -> bool {
    WINDOW_MOTION_GENERATION.load(Ordering::SeqCst) == generation
}

#[cfg(windows)]
fn release_velocity(
    samples: &std::collections::VecDeque<(std::time::Instant, i32, i32)>,
) -> (f64, f64) {
    let Some((first_time, first_x, first_y)) = samples.front().copied() else {
        return (0.0, 0.0);
    };
    let Some((last_time, last_x, last_y)) = samples.back().copied() else {
        return (0.0, 0.0);
    };

    let dt = last_time
        .saturating_duration_since(first_time)
        .as_secs_f64();
    if dt < 0.012 {
        return (0.0, 0.0);
    }

    (
        (last_x - first_x) as f64 / dt,
        (last_y - first_y) as f64 / dt,
    )
}

#[cfg(windows)]
fn cursor_position() -> Option<windows::Win32::Foundation::POINT> {
    use windows::Win32::{Foundation::POINT, UI::WindowsAndMessaging::GetCursorPos};

    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok()?;
    Some(point)
}

#[cfg(windows)]
fn left_button_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};

    (unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) }) < 0
}

#[cfg(windows)]
#[derive(Clone, Copy)]
struct MonitorWorkArea {
    handle: windows::Win32::Graphics::Gdi::HMONITOR,
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

#[cfg(windows)]
fn monitor_work_area(x: i32, y: i32) -> Option<MonitorWorkArea> {
    use std::mem::size_of;
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST},
    };

    let handle = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) };
    if handle.0.is_null() {
        return None;
    }

    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(handle, &mut info) }.ok()?;

    Some(MonitorWorkArea {
        handle,
        left: info.rcWork.left as f64,
        top: info.rcWork.top as f64,
        right: info.rcWork.right as f64,
        bottom: info.rcWork.bottom as f64,
    })
}

#[cfg(windows)]
fn other_monitor_at(current: windows::Win32::Graphics::Gdi::HMONITOR, x: i32, y: i32) -> bool {
    use windows::Win32::{
        Foundation::POINT,
        Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONULL},
    };

    let candidate = unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONULL) };
    !candidate.0.is_null() && candidate != current
}

#[cfg(windows)]
fn resolve_desktop_collisions(
    x: &mut f64,
    y: &mut f64,
    width: f64,
    height: f64,
    vx: &mut f64,
    vy: &mut f64,
    bounce: f64,
) {
    let center_x = (*x + width * 0.5).round() as i32;
    let center_y = (*y + height * 0.5).round() as i32;
    let Some(area) = monitor_work_area(center_x, center_y) else {
        return;
    };

    let max_x = (area.right - width).max(area.left);
    let max_y = (area.bottom - height).max(area.top);

    if *vx < 0.0 && *x < area.left {
        let probe_x = area.left.round() as i32 - 1;
        if !other_monitor_at(area.handle, probe_x, center_y) {
            *x = area.left;
            *vx = vx.abs() * bounce;
        }
    } else if *vx > 0.0 && *x > max_x {
        let probe_x = area.right.round() as i32 + 1;
        if !other_monitor_at(area.handle, probe_x, center_y) {
            *x = max_x;
            *vx = -vx.abs() * bounce;
        }
    }

    let center_x = (*x + width * 0.5).round() as i32;
    if *vy < 0.0 && *y < area.top {
        let probe_y = area.top.round() as i32 - 1;
        if !other_monitor_at(area.handle, center_x, probe_y) {
            *y = area.top;
            *vy = vy.abs() * bounce;
        }
    } else if *vy > 0.0 && *y > max_y {
        let probe_y = area.bottom.round() as i32 + 1;
        if !other_monitor_at(area.handle, center_x, probe_y) {
            *y = max_y;
            *vy = -vy.abs() * bounce;
        }
    }
}
