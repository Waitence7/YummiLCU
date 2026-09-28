use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

use tauri::{AppHandle, Manager, State};

use crate::{state::AppState, tray};

static WINDOW_MOTION_GENERATION: AtomicU64 = AtomicU64::new(0);
static WINDOW_ROTATION_BITS: AtomicU64 = AtomicU64::new(0.0f64.to_bits());

const MAIN_CONTENT_WIDTH_LOGICAL: f64 = 640.0;
const MAIN_CONTENT_HEIGHT_LOGICAL: f64 = 620.0;
const ROTATION_HOST_SIZE_LOGICAL: f64 = 960.0;
const ROTATION_REGION_PADDING_LOGICAL: f64 = 24.0;

fn cancel_window_motion() {
    WINDOW_MOTION_GENERATION.fetch_add(1, Ordering::SeqCst);
}

fn current_rotation_angle() -> f64 {
    f64::from_bits(WINDOW_ROTATION_BITS.load(Ordering::SeqCst))
}

fn store_rotation_angle(angle: f64) {
    WINDOW_ROTATION_BITS.store(angle.to_bits(), Ordering::SeqCst);
}

#[tauri::command]
pub(crate) fn hide_main_window(app: AppHandle) {
    cancel_window_motion();
    store_rotation_angle(0.0);
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window("main") {
        let _ = set_rotation_host_expanded(&window, false);
        emit_motion_visual(&window, 0.0, 0.0, 0.0, 0.0, "stop");
    }
    tray::hide_main_window(&app);
}

#[tauri::command]
pub(crate) fn complete_tray_hide(app: AppHandle) {
    cancel_window_motion();
    store_rotation_angle(0.0);
    tray::hide_main_window(&app);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(80)).await;
        tray::destroy_main_window(&app);
    });
}

#[tauri::command]
pub(crate) fn minimize_main_window(app: AppHandle) -> Result<(), String> {
    cancel_window_motion();
    store_rotation_angle(0.0);
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "메인 창을 찾을 수 없습니다.".to_string())?;
    #[cfg(windows)]
    {
        set_rotation_host_expanded(&window, false)?;
        emit_motion_visual(&window, 0.0, 0.0, 0.0, 0.0, "stop");
    }
    window.minimize().map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn request_tray_hide(app: AppHandle) {
    cancel_window_motion();
    store_rotation_angle(0.0);
    tray::request_animated_hide(&app);
}

#[tauri::command]
pub(crate) async fn start_main_window_drag(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "메인 창을 찾을 수 없습니다.".to_string())?;

    #[cfg(windows)]
    {
        let config = state.config.read().await.clone();
        let generation = WINDOW_MOTION_GENERATION
            .fetch_add(1, Ordering::SeqCst)
            .wrapping_add(1);

        // Do not call SetWindowRgn while the left button is down. Changing the
        // top-level HWND region in the middle of a WebView2 pointer sequence can
        // cancel the matching mouse-up/click and make the custom drag look dead.
        std::thread::Builder::new()
            .name("yummi-window-physics".into())
            .spawn(move || {
                run_windows_physics_drag(
                    window,
                    generation,
                    config.window_glide_strength,
                    config.window_free_rotation,
                )
            })
            .map(|_| ())
            .map_err(|error| format!("창 물리 스레드를 시작하지 못했습니다: {error}"))
    }

    #[cfg(not(windows))]
    {
        let _ = state;
        cancel_window_motion();
        window.start_dragging().map_err(|error| error.to_string())
    }
}

#[tauri::command]
pub(crate) async fn sync_main_window_rotation_mode(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> Result<(), String> {
    let enabled = state.config.read().await.window_free_rotation;
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "메인 창을 찾을 수 없습니다.".to_string())?;

    cancel_window_motion();
    store_rotation_angle(0.0);

    #[cfg(windows)]
    {
        // Merely enabling free rotation must not enlarge the transparent WebView.
        // Keep the normal compact window until an actual angular throw begins.
        let _ = enabled;
        set_rotation_host_expanded(&window, false)?;
        emit_motion_visual(&window, 0.0, 0.0, 0.0, 0.0, "stop");
    }

    #[cfg(not(windows))]
    {
        let _ = enabled;
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn freeze_main_window_motion() {
    // Safe during pointer-down: only invalidate the physics generation.
    // The current rotated HWND region stays untouched until click completes.
    cancel_window_motion();
}

#[tauri::command]
pub(crate) fn stabilize_main_window_rotation(app: AppHandle) -> Result<(), String> {
    let Some(window) = app.get_webview_window("main") else {
        return Ok(());
    };
    cancel_window_motion();
    store_rotation_angle(0.0);

    #[cfg(windows)]
    {
        // This runs after the click has fully dispatched, so one native region
        // update is safe. Always restore the compact upright region in case the
        // cancelled spin was using the larger fixed rotation envelope.
        set_rotation_now(&window, 0.0)?;
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn show_window_glide_hint(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {
        use tauri::{WebviewUrl, WebviewWindowBuilder};

        if let Some(window) = app.get_webview_window("motion-hint") {
            position_window_glide_hint(&window)?;
            window.show().map_err(|error| error.to_string())?;
            return Ok(());
        }

        let window =
            WebviewWindowBuilder::new(&app, "motion-hint", WebviewUrl::App("index.html".into()))
                .title("창 미끄러짐")
                .inner_size(360.0, 190.0)
                .min_inner_size(360.0, 190.0)
                .max_inner_size(360.0, 190.0)
                .resizable(false)
                .decorations(false)
                .transparent(true)
                .shadow(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .focused(false)
                .visible(false)
                .build()
                .map_err(|error| format!("창 미끄러짐 안내를 열지 못했습니다: {error}"))?;

        position_window_glide_hint(&window)?;
        window.show().map_err(|error| error.to_string())?;
    }

    #[cfg(not(windows))]
    {
        let _ = app;
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn close_window_glide_hint(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("motion-hint") {
        window.close().map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[cfg(windows)]
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct WindowMotionVisual {
    vx: f64,
    vy: f64,
    speed: f64,
    angle: f64,
    angular_velocity: f64,
    phase: &'static str,
}

#[cfg(windows)]
fn emit_motion_visual(
    window: &tauri::WebviewWindow,
    vx: f64,
    vy: f64,
    angle: f64,
    angular_velocity: f64,
    phase: &'static str,
) {
    use tauri::Emitter;

    let _ = window.emit(
        "yummi://window-motion",
        WindowMotionVisual {
            vx,
            vy,
            speed: vx.hypot(vy),
            angle,
            angular_velocity,
            phase,
        },
    );
}

#[cfg(windows)]
struct MotionVisualGuard {
    window: tauri::WebviewWindow,
    generation: u64,
}

#[cfg(windows)]
impl Drop for MotionVisualGuard {
    fn drop(&mut self) {
        if motion_is_current(self.generation) {
            emit_motion_visual(
                &self.window,
                0.0,
                0.0,
                current_rotation_angle(),
                0.0,
                "stop",
            );
        }
    }
}

#[cfg(windows)]
fn run_windows_physics_drag(
    window: tauri::WebviewWindow,
    generation: u64,
    glide_strength: Option<f64>,
    free_rotation: bool,
) {
    use std::{
        collections::VecDeque,
        thread,
        time::{Duration, Instant},
    };

    use tauri::{PhysicalPosition, Position};

    const DRAG_INTERVAL: Duration = Duration::from_millis(8);
    const ROTATION_PHYSICS_INTERVAL: Duration = Duration::from_millis(16);
    const ROTATION_PRESENT_INTERVAL: Duration = Duration::from_millis(33);
    const IDLE_ROTATION_INTERVAL: Duration = Duration::from_millis(40);
    const VISUAL_INTERVAL: Duration = Duration::from_millis(16);
    const VELOCITY_SAMPLE_WINDOW: Duration = Duration::from_millis(64);
    const CURSOR_HISTORY_WINDOW: Duration = Duration::from_millis(120);
    const FRICTION_PER_60HZ_FRAME: f64 = 0.965;
    const ANGULAR_FRICTION_PER_60HZ_FRAME: f64 = 0.985;
    const BOUNCE: f64 = 0.68;
    const STOP_SPEED_PX_S: f64 = 22.0;
    const STOP_ANGULAR_SPEED_DEG_S: f64 = 7.0;
    const MIN_THROW_SPEED_PX_S: f64 = 55.0;
    const MIN_THROW_ANGULAR_SPEED_DEG_S: f64 = 18.0;
    const LOW_SPEED_HOVER_PX_S: f64 = 90.0;
    const LOW_SPEED_HOVER_ANGULAR_DEG_S: f64 = 80.0;
    const HOVER_CURSOR_TRAVEL_PX: f64 = 4.0;
    const THROW_SOFT_KNEE_PX_S: f64 = 3600.0;
    const THROW_SOFT_SPAN_PX_S: f64 = 5400.0;
    const MAX_ANGULAR_SPEED_DEG_S: f64 = 1800.0;
    const ANGULAR_TORQUE_SCALE: f64 = 0.34;
    const SETTLE_RATE: f64 = 15.0;

    if !motion_is_current(generation) || !left_button_down() {
        return;
    }

    let Some(initial_cursor) = cursor_position() else {
        return;
    };
    let Ok(initial_position) = window.outer_position() else {
        return;
    };
    let Ok(initial_size) = window.outer_size() else {
        return;
    };
    let scale = window.scale_factor().unwrap_or(1.0);

    let _visual_guard = MotionVisualGuard {
        window: window.clone(),
        generation,
    };

    let offset_x = initial_position.x - initial_cursor.x;
    let offset_y = initial_position.y - initial_cursor.y;
    let window_center_x = initial_position.x as f64 + initial_size.width as f64 * 0.5;
    let window_center_y = initial_position.y as f64 + initial_size.height as f64 * 0.5;
    let grab_x = (initial_cursor.x as f64 - window_center_x)
        / (MAIN_CONTENT_WIDTH_LOGICAL * scale * 0.5).max(1.0);
    let grab_y = (initial_cursor.y as f64 - window_center_y)
        / (MAIN_CONTENT_HEIGHT_LOGICAL * scale * 0.5).max(1.0);

    let initial_now = Instant::now();
    let mut samples = VecDeque::with_capacity(16);
    samples.push_back((initial_now, initial_position.x, initial_position.y));
    let mut last_visual_emit = initial_now - VISUAL_INTERVAL;

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
        trim_motion_samples(&mut samples, now, VELOCITY_SAMPLE_WINDOW);

        if now.saturating_duration_since(last_visual_emit) >= VISUAL_INTERVAL {
            let (vx, vy) = release_velocity(&samples);
            emit_motion_visual(&window, vx, vy, 0.0, 0.0, "drag");
            last_visual_emit = now;
        }

        thread::sleep(DRAG_INTERVAL);
    }

    if !motion_is_current(generation) {
        return;
    }

    if let Some(cursor) = cursor_position() {
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
        trim_motion_samples(&mut samples, now, VELOCITY_SAMPLE_WINDOW);
    }

    let raw_velocity = release_velocity(&samples);
    let (mut vx, mut vy) = soften_throw_velocity(
        raw_velocity.0,
        raw_velocity.1,
        THROW_SOFT_KNEE_PX_S,
        THROW_SOFT_SPAN_PX_S,
    );

    let mut angular_velocity = if free_rotation {
        ((grab_x * raw_velocity.1 - grab_y * raw_velocity.0) * ANGULAR_TORQUE_SCALE)
            .clamp(-MAX_ANGULAR_SPEED_DEG_S, MAX_ANGULAR_SPEED_DEG_S)
    } else {
        0.0
    };

    if matches!(glide_strength, Some(value) if value <= f64::EPSILON) {
        vx = 0.0;
        vy = 0.0;
    }

    if vx.hypot(vy) < MIN_THROW_SPEED_PX_S {
        vx = 0.0;
        vy = 0.0;
    }
    if angular_velocity.abs() < MIN_THROW_ANGULAR_SPEED_DEG_S {
        angular_velocity = 0.0;
    }

    let rotation_host_expanded = free_rotation && angular_velocity != 0.0;
    if rotation_host_expanded && set_rotation_host_expanded(&window, true).is_err() {
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
    let host_width = size.width as f64;
    let host_height = size.height as f64;

    if vx == 0.0 && vy == 0.0 && angular_velocity == 0.0 {
        return;
    }

    let mut angle = 0.0;
    store_rotation_angle(angle);
    let mut previous = Instant::now();
    let mut last_window_move = previous - DRAG_INTERVAL;
    let mut settling = false;
    let mut settle_target = 0.0;
    let mut cursor_history: VecDeque<(Instant, i32, i32)> = VecDeque::with_capacity(24);
    let mut previous_cursor_inside = false;
    let mut idle_region_applied = false;

    loop {
        if !motion_is_current(generation) {
            return;
        }

        let now = Instant::now();
        let dt = now
            .saturating_duration_since(previous)
            .as_secs_f64()
            .clamp(0.001, 0.050);
        previous = now;

        if settling {
            vx = 0.0;
            vy = 0.0;
            angular_velocity = 0.0;
            let delta = settle_target - angle;
            let amount = 1.0 - (-SETTLE_RATE * dt).exp();
            angle += delta * amount;

            if delta.abs() <= 0.08 {
                angle = settle_target;
                store_rotation_angle(angle);
                let _ = set_rotation_host_expanded(&window, false);
                emit_motion_visual(&window, 0.0, 0.0, angle, 0.0, "stop");
                return;
            }
        } else {
            x += vx * dt;
            y += vy * dt;
            angle += angular_velocity * dt;
            store_rotation_angle(angle);

            let (visual_width, visual_height) =
                rotated_visual_bounds(scale, if free_rotation { angle } else { 0.0 });
            resolve_desktop_collisions(
                &mut x,
                &mut y,
                host_width,
                host_height,
                visual_width,
                visual_height,
                &mut vx,
                &mut vy,
                BOUNCE,
            );

            let move_interval = if rotation_host_expanded {
                ROTATION_PRESENT_INTERVAL
            } else {
                DRAG_INTERVAL
            };
            if now.saturating_duration_since(last_window_move) >= move_interval {
                if window
                    .set_position(Position::Physical(PhysicalPosition::new(
                        x.round() as i32,
                        y.round() as i32,
                    )))
                    .is_err()
                {
                    return;
                }
                last_window_move = now;
            }

            if let Some(strength) = glide_strength {
                let friction =
                    FRICTION_PER_60HZ_FRAME.powf(dt / (1.0 / 60.0) / strength.max(0.001));
                vx *= friction;
                vy *= friction;
            }

            if free_rotation {
                let angular_friction = ANGULAR_FRICTION_PER_60HZ_FRAME.powf(dt / (1.0 / 60.0));
                angular_velocity *= angular_friction;
            } else {
                angular_velocity = 0.0;
                angle = 0.0;
            }

            if vx.hypot(vy) < STOP_SPEED_PX_S {
                vx = 0.0;
                vy = 0.0;
            }
            if angular_velocity.abs() < STOP_ANGULAR_SPEED_DEG_S {
                angular_velocity = 0.0;
            }

            if free_rotation {
                if let Some(cursor) = cursor_position() {
                    cursor_history.push_back((now, cursor.x, cursor.y));
                    while cursor_history
                        .front()
                        .map(|(time, _, _)| {
                            now.saturating_duration_since(*time) > CURSOR_HISTORY_WINDOW
                        })
                        .unwrap_or(false)
                    {
                        cursor_history.pop_front();
                    }

                    let inside =
                        point_inside_rotated_surface(&window, cursor.x, cursor.y, angle, scale);

                    if vx.hypot(vy) <= LOW_SPEED_HOVER_PX_S
                        && angular_velocity.abs() <= LOW_SPEED_HOVER_ANGULAR_DEG_S
                        && inside
                        && !previous_cursor_inside
                        && cursor_approached_window(
                            &cursor_history,
                            x + host_width * 0.5,
                            y + host_height * 0.5,
                            HOVER_CURSOR_TRAVEL_PX,
                        )
                    {
                        // If an idle tilted window had been tightened to a polygon,
                        // reopen the fixed envelope once before the settle animation.
                        let _ = apply_rotation_envelope_region(&window);
                        settling = true;
                        settle_target = nearest_upright_angle(angle);
                        vx = 0.0;
                        vy = 0.0;
                    }
                    previous_cursor_inside = inside;
                }
            }
        }

        let visual_interval = if rotation_host_expanded {
            ROTATION_PRESENT_INTERVAL
        } else {
            VISUAL_INTERVAL
        };
        if now.saturating_duration_since(last_visual_emit) >= visual_interval {
            emit_motion_visual(
                &window,
                vx,
                vy,
                angle,
                angular_velocity,
                if settling { "settle" } else { "glide" },
            );
            last_visual_emit = now;
        }

        let idle_rotated = free_rotation
            && !settling
            && vx == 0.0
            && vy == 0.0
            && angular_velocity == 0.0
            && distance_to_upright(angle) > 0.08;

        if idle_rotated && !idle_region_applied {
            // Motion is fully stopped, so one final tight polygon is cheap and
            // reduces the transparent hit area while the tilted window idles.
            let _ = apply_rotation_region(&window, angle);
            idle_region_applied = true;
        } else if !idle_rotated {
            idle_region_applied = false;
        }

        if !idle_rotated && !settling && vx == 0.0 && vy == 0.0 && angular_velocity == 0.0 {
            if rotation_host_expanded {
                let _ = set_rotation_host_expanded(&window, false);
            }
            return;
        }

        let interval = if idle_rotated {
            IDLE_ROTATION_INTERVAL
        } else if rotation_host_expanded {
            ROTATION_PHYSICS_INTERVAL
        } else {
            DRAG_INTERVAL
        };
        let spent = Instant::now().saturating_duration_since(now);
        if spent < interval {
            thread::sleep(interval - spent);
        }
    }
}

#[cfg(windows)]
fn trim_motion_samples(
    samples: &mut std::collections::VecDeque<(std::time::Instant, i32, i32)>,
    now: std::time::Instant,
    max_age: std::time::Duration,
) {
    while samples
        .front()
        .map(|(time, _, _)| now.saturating_duration_since(*time) > max_age)
        .unwrap_or(false)
    {
        samples.pop_front();
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
    use std::time::Duration;

    const MIN_SAMPLE_SPAN: Duration = Duration::from_millis(12);
    const SHORT_WINDOW: Duration = Duration::from_millis(28);
    const WEIGHT_HALF_LIFE_S: f64 = 0.018;
    const SHORT_BLEND: f64 = 0.65;

    let Some((first_time, _, _)) = samples.front().copied() else {
        return (0.0, 0.0);
    };
    let Some((last_time, last_x, last_y)) = samples.back().copied() else {
        return (0.0, 0.0);
    };
    if last_time.saturating_duration_since(first_time) < MIN_SAMPLE_SPAN {
        return (0.0, 0.0);
    }

    let mut weight_sum = 0.0;
    let mut weighted_t = 0.0;
    let mut weighted_x = 0.0;
    let mut weighted_y = 0.0;
    for (time, x, y) in samples {
        let age = last_time.saturating_duration_since(*time).as_secs_f64();
        let t = -age;
        let weight = 0.5_f64.powf(age / WEIGHT_HALF_LIFE_S);
        weight_sum += weight;
        weighted_t += weight * t;
        weighted_x += weight * *x as f64;
        weighted_y += weight * *y as f64;
    }
    if weight_sum <= f64::EPSILON {
        return (0.0, 0.0);
    }

    let mean_t = weighted_t / weight_sum;
    let mean_x = weighted_x / weight_sum;
    let mean_y = weighted_y / weight_sum;
    let mut variance_t = 0.0;
    let mut covariance_x = 0.0;
    let mut covariance_y = 0.0;

    for (time, x, y) in samples {
        let age = last_time.saturating_duration_since(*time).as_secs_f64();
        let t = -age;
        let weight = 0.5_f64.powf(age / WEIGHT_HALF_LIFE_S);
        let centered_t = t - mean_t;
        variance_t += weight * centered_t * centered_t;
        covariance_x += weight * centered_t * (*x as f64 - mean_x);
        covariance_y += weight * centered_t * (*y as f64 - mean_y);
    }
    if variance_t <= 1e-9 {
        return (0.0, 0.0);
    }

    let regression_vx = covariance_x / variance_t;
    let regression_vy = covariance_y / variance_t;
    let short_first = samples
        .iter()
        .find(|(time, _, _)| last_time.saturating_duration_since(*time) <= SHORT_WINDOW)
        .copied()
        .unwrap_or((first_time, last_x, last_y));
    let short_dt = last_time
        .saturating_duration_since(short_first.0)
        .as_secs_f64();

    if short_dt < 0.008 {
        return (regression_vx, regression_vy);
    }

    let short_vx = (last_x - short_first.1) as f64 / short_dt;
    let short_vy = (last_y - short_first.2) as f64 / short_dt;
    (
        regression_vx * (1.0 - SHORT_BLEND) + short_vx * SHORT_BLEND,
        regression_vy * (1.0 - SHORT_BLEND) + short_vy * SHORT_BLEND,
    )
}

#[cfg(windows)]
fn soften_throw_velocity(vx: f64, vy: f64, knee: f64, span: f64) -> (f64, f64) {
    let speed = vx.hypot(vy);
    if speed <= knee || span <= 0.0 {
        return (vx, vy);
    }
    let mapped = knee + span * ((speed - knee) / span).tanh();
    let scale = mapped / speed;
    (vx * scale, vy * scale)
}

#[cfg(windows)]
fn nearest_upright_angle(angle: f64) -> f64 {
    (angle / 360.0).round() * 360.0
}

#[cfg(windows)]
fn distance_to_upright(angle: f64) -> f64 {
    (nearest_upright_angle(angle) - angle).abs()
}

#[cfg(windows)]
fn rotated_visual_bounds(scale: f64, angle: f64) -> (f64, f64) {
    let width = MAIN_CONTENT_WIDTH_LOGICAL * scale;
    let height = MAIN_CONTENT_HEIGHT_LOGICAL * scale;
    let radians = angle.to_radians();
    let cos = radians.cos().abs();
    let sin = radians.sin().abs();
    (width * cos + height * sin, width * sin + height * cos)
}

#[cfg(windows)]
fn cursor_approached_window(
    history: &std::collections::VecDeque<(std::time::Instant, i32, i32)>,
    center_x: f64,
    center_y: f64,
    min_travel: f64,
) -> bool {
    let Some((_, first_x, first_y)) = history.front().copied() else {
        return false;
    };
    let Some((_, last_x, last_y)) = history.back().copied() else {
        return false;
    };

    let dx = (last_x - first_x) as f64;
    let dy = (last_y - first_y) as f64;
    if dx.hypot(dy) < min_travel {
        return false;
    }

    let toward_x = center_x - first_x as f64;
    let toward_y = center_y - first_y as f64;
    dx * toward_x + dy * toward_y > 0.0
}

#[cfg(windows)]
fn point_inside_rotated_surface(
    window: &tauri::WebviewWindow,
    cursor_x: i32,
    cursor_y: i32,
    angle: f64,
    scale: f64,
) -> bool {
    let Ok(position) = window.outer_position() else {
        return false;
    };
    let Ok(size) = window.outer_size() else {
        return false;
    };

    let center_x = position.x as f64 + size.width as f64 * 0.5;
    let center_y = position.y as f64 + size.height as f64 * 0.5;
    let dx = cursor_x as f64 - center_x;
    let dy = cursor_y as f64 - center_y;
    let radians = angle.to_radians();
    let cos = radians.cos();
    let sin = radians.sin();
    let local_x = cos * dx + sin * dy;
    let local_y = -sin * dx + cos * dy;
    let half_width = MAIN_CONTENT_WIDTH_LOGICAL * scale * 0.5;
    let half_height = MAIN_CONTENT_HEIGHT_LOGICAL * scale * 0.5;

    local_x.abs() <= half_width && local_y.abs() <= half_height
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
fn set_rotation_now(window: &tauri::WebviewWindow, angle: f64) -> Result<(), String> {
    store_rotation_angle(angle);
    if distance_to_upright(angle) <= 0.08 {
        set_rotation_host_expanded(window, false)?;
    } else {
        apply_rotation_region(window, angle)?;
    }
    emit_motion_visual(window, 0.0, 0.0, angle, 0.0, "stop");
    Ok(())
}

#[cfg(windows)]
fn set_rotation_host_expanded(window: &tauri::WebviewWindow, expanded: bool) -> Result<(), String> {
    use tauri::{LogicalSize, PhysicalPosition, Position};

    let old_position = window.outer_position().map_err(|error| error.to_string())?;
    let old_size = window.outer_size().map_err(|error| error.to_string())?;
    let center_x = old_position.x as f64 + old_size.width as f64 * 0.5;
    let center_y = old_position.y as f64 + old_size.height as f64 * 0.5;
    let scale = window.scale_factor().unwrap_or(1.0);
    let target_width = if expanded {
        ROTATION_HOST_SIZE_LOGICAL
    } else {
        MAIN_CONTENT_WIDTH_LOGICAL
    };
    let target_height = if expanded {
        ROTATION_HOST_SIZE_LOGICAL
    } else {
        MAIN_CONTENT_HEIGHT_LOGICAL
    };
    let target_physical_width = target_width * scale;
    let target_physical_height = target_height * scale;
    let already_sized = (old_size.width as f64 - target_physical_width).abs() <= 2.0
        && (old_size.height as f64 - target_physical_height).abs() <= 2.0;

    if !expanded {
        // Clear any previous envelope/tight region before shrinking the HWND.
        clear_window_region(window)?;
    }

    if !already_sized {
        window
            .set_size(LogicalSize::new(target_width, target_height))
            .map_err(|error| error.to_string())?;

        let new_size = window.outer_size().map_err(|error| error.to_string())?;
        window
            .set_position(Position::Physical(PhysicalPosition::new(
                (center_x - new_size.width as f64 * 0.5).round() as i32,
                (center_y - new_size.height as f64 * 0.5).round() as i32,
            )))
            .map_err(|error| error.to_string())?;
    }

    if expanded {
        apply_rotation_envelope_region(window)?;
    }
    Ok(())
}

#[cfg(windows)]
fn apply_rotation_envelope_region(window: &tauri::WebviewWindow) -> Result<(), String> {
    use windows::Win32::{
        Foundation::HWND,
        Graphics::Gdi::{CreateEllipticRgn, DeleteObject, SetWindowRgn, HGDIOBJ},
    };

    let native = window.hwnd().map_err(|error| error.to_string())?;
    let hwnd = HWND(native.0);
    let inner = window.inner_size().map_err(|error| error.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let center_x = inner.width as f64 * 0.5;
    let center_y = inner.height as f64 * 0.5;
    let half_width = MAIN_CONTENT_WIDTH_LOGICAL * scale * 0.5;
    let half_height = MAIN_CONTENT_HEIGHT_LOGICAL * scale * 0.5;
    let radius = half_width.hypot(half_height) + ROTATION_REGION_PADDING_LOGICAL * scale;

    let region = unsafe {
        CreateEllipticRgn(
            (center_x - radius).floor() as i32,
            (center_y - radius).floor() as i32,
            (center_x + radius).ceil() as i32,
            (center_y + radius).ceil() as i32,
        )
    };
    if region.0.is_null() {
        return Err("회전 안전 영역 생성에 실패했습니다.".into());
    }
    if unsafe { SetWindowRgn(hwnd, Some(region), true) } == 0 {
        let _ = unsafe { DeleteObject(HGDIOBJ(region.0)) };
        return Err("회전 안전 영역 적용에 실패했습니다.".into());
    }
    Ok(())
}

#[cfg(windows)]
fn apply_rotation_region(window: &tauri::WebviewWindow, angle: f64) -> Result<(), String> {
    use windows::Win32::{
        Foundation::{HWND, POINT},
        Graphics::Gdi::{CreatePolygonRgn, DeleteObject, SetWindowRgn, HGDIOBJ, WINDING},
    };

    let native = window.hwnd().map_err(|error| error.to_string())?;
    let hwnd = HWND(native.0);
    let inner = window.inner_size().map_err(|error| error.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let center_x = inner.width as f64 * 0.5;
    let center_y = inner.height as f64 * 0.5;
    let half_width = (MAIN_CONTENT_WIDTH_LOGICAL * 0.5 + ROTATION_REGION_PADDING_LOGICAL) * scale;
    let half_height = (MAIN_CONTENT_HEIGHT_LOGICAL * 0.5 + ROTATION_REGION_PADDING_LOGICAL) * scale;
    let radians = angle.to_radians();
    let cos = radians.cos();
    let sin = radians.sin();

    let points = [
        (-half_width, -half_height),
        (half_width, -half_height),
        (half_width, half_height),
        (-half_width, half_height),
    ]
    .map(|(x, y)| POINT {
        x: (center_x + x * cos - y * sin).round() as i32,
        y: (center_y + x * sin + y * cos).round() as i32,
    });

    let region = unsafe { CreatePolygonRgn(&points, WINDING) };
    if region.0.is_null() {
        return Err("회전 창 영역 생성에 실패했습니다.".into());
    }

    if unsafe { SetWindowRgn(hwnd, Some(region), true) } == 0 {
        let _ = unsafe { DeleteObject(HGDIOBJ(region.0)) };
        return Err("회전 창 영역 적용에 실패했습니다.".into());
    }
    Ok(())
}

#[cfg(windows)]
fn clear_window_region(window: &tauri::WebviewWindow) -> Result<(), String> {
    use windows::Win32::{Foundation::HWND, Graphics::Gdi::SetWindowRgn};

    let native = window.hwnd().map_err(|error| error.to_string())?;
    let hwnd = HWND(native.0);
    if unsafe { SetWindowRgn(hwnd, None, true) } == 0 {
        return Err("창 영역 초기화에 실패했습니다.".into());
    }
    Ok(())
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
    if !unsafe { GetMonitorInfoW(handle, &mut info) }.as_bool() {
        return None;
    }

    Some(MonitorWorkArea {
        handle,
        left: info.rcWork.left as f64,
        top: info.rcWork.top as f64,
        right: info.rcWork.right as f64,
        bottom: info.rcWork.bottom as f64,
    })
}

#[cfg(windows)]
fn position_window_glide_hint(window: &tauri::WebviewWindow) -> Result<(), String> {
    use tauri::{PhysicalPosition, Position};

    const MARGIN_LOGICAL: f64 = 16.0;

    let cursor =
        cursor_position().ok_or_else(|| "마우스 위치를 확인할 수 없습니다.".to_string())?;
    let area = monitor_work_area(cursor.x, cursor.y)
        .ok_or_else(|| "현재 모니터 영역을 확인할 수 없습니다.".to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0);
    let size = window.outer_size().map_err(|error| error.to_string())?;
    let margin = (MARGIN_LOGICAL * scale).round() as i32;
    let x = area.right.round() as i32 - size.width as i32 - margin;
    let y = area.bottom.round() as i32 - size.height as i32 - margin;

    window
        .set_position(Position::Physical(PhysicalPosition::new(x, y)))
        .map_err(|error| error.to_string())
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
    host_width: f64,
    host_height: f64,
    visual_width: f64,
    visual_height: f64,
    vx: &mut f64,
    vy: &mut f64,
    bounce: f64,
) {
    let center_x = (*x + host_width * 0.5).round() as i32;
    let center_y = (*y + host_height * 0.5).round() as i32;
    let Some(area) = monitor_work_area(center_x, center_y) else {
        return;
    };

    let offset_x = (host_width - visual_width) * 0.5;
    let offset_y = (host_height - visual_height) * 0.5;
    let visual_left = *x + offset_x;
    let visual_top = *y + offset_y;
    let visual_right = visual_left + visual_width;
    let visual_bottom = visual_top + visual_height;

    if *vx < 0.0 && visual_left < area.left {
        let probe_x = area.left.round() as i32 - 1;
        if !other_monitor_at(area.handle, probe_x, center_y) {
            *x += area.left - visual_left;
            *vx = vx.abs() * bounce;
        }
    } else if *vx > 0.0 && visual_right > area.right {
        let probe_x = area.right.round() as i32 + 1;
        if !other_monitor_at(area.handle, probe_x, center_y) {
            *x -= visual_right - area.right;
            *vx = -vx.abs() * bounce;
        }
    }

    let center_x = (*x + host_width * 0.5).round() as i32;
    if *vy < 0.0 && visual_top < area.top {
        let probe_y = area.top.round() as i32 - 1;
        if !other_monitor_at(area.handle, center_x, probe_y) {
            *y += area.top - visual_top;
            *vy = vy.abs() * bounce;
        }
    } else if *vy > 0.0 && visual_bottom > area.bottom {
        let probe_y = area.bottom.round() as i32 + 1;
        if !other_monitor_at(area.handle, center_x, probe_y) {
            *y -= visual_bottom - area.bottom;
            *vy = -vy.abs() * bounce;
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::{
        cursor_approached_window, nearest_upright_angle, release_velocity, rotated_visual_bounds,
        soften_throw_velocity,
    };
    use std::{
        collections::VecDeque,
        time::{Duration, Instant},
    };

    #[test]
    fn release_velocity_favors_a_fast_final_flick() {
        let start = Instant::now();
        let mut samples = VecDeque::new();
        let points = [
            (0, 0),
            (8, 6),
            (16, 13),
            (24, 19),
            (32, 26),
            (40, 32),
            (48, 38),
            (56, 78),
            (64, 118),
        ];
        for (millis, x) in points {
            samples.push_back((start + Duration::from_millis(millis), x, 0));
        }

        let (vx, vy) = release_velocity(&samples);
        assert!(vx > 2_500.0, "expected recent flick to dominate, got {vx}");
        assert!(vy.abs() < 1.0);
    }

    #[test]
    fn release_velocity_remains_close_for_constant_motion() {
        let start = Instant::now();
        let mut samples = VecDeque::new();
        for index in 0..=8 {
            let millis = index * 8;
            let x = (1_800.0 * (millis as f64 / 1_000.0)).round() as i32;
            samples.push_back((start + Duration::from_millis(millis), x, 0));
        }

        let (vx, _) = release_velocity(&samples);
        assert!(
            (vx - 1_800.0).abs() < 120.0,
            "unexpected constant velocity {vx}"
        );
    }

    #[test]
    fn throw_soft_limit_preserves_fast_speed_differences() {
        let (vx_5k, _) = soften_throw_velocity(5_000.0, 0.0, 3_600.0, 5_400.0);
        let (vx_8k, _) = soften_throw_velocity(8_000.0, 0.0, 3_600.0, 5_400.0);

        assert!(vx_5k > 4_500.0);
        assert!(vx_8k > vx_5k + 1_000.0);
        assert!(vx_8k < 9_000.0);
    }

    #[test]
    fn upright_target_uses_nearest_full_turn() {
        assert_eq!(nearest_upright_angle(710.0), 720.0);
        assert_eq!(nearest_upright_angle(-380.0), -360.0);
        assert_eq!(nearest_upright_angle(190.0), 360.0);
    }

    #[test]
    fn rotated_bounds_fit_square_at_quarter_turn() {
        let (width, height) = rotated_visual_bounds(1.0, 90.0);
        assert!((width - 620.0).abs() < 0.001);
        assert!((height - 640.0).abs() < 0.001);
    }

    #[test]
    fn hover_approach_requires_cursor_motion_toward_window() {
        let start = Instant::now();
        let history = VecDeque::from([
            (start, 100, 100),
            (start + Duration::from_millis(100), 112, 108),
        ]);
        assert!(cursor_approached_window(&history, 300.0, 300.0, 4.0));

        let stationary = VecDeque::from([
            (start, 100, 100),
            (start + Duration::from_millis(100), 101, 100),
        ]);
        assert!(!cursor_approached_window(&stationary, 300.0, 300.0, 4.0));
    }
}
