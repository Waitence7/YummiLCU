use std::sync::{
    atomic::{AtomicBool, AtomicU64, Ordering},
    Arc,
};

use tauri::{AppHandle, Manager, State};

use crate::{state::AppState, tray};

static WINDOW_MOTION_GENERATION: AtomicU64 = AtomicU64::new(0);
static WINDOW_ROTATION_BITS: AtomicU64 = AtomicU64::new(0.0f64.to_bits());
static WINDOW_ROTATION_HOST_EXPANDED: AtomicBool = AtomicBool::new(false);
static WINDOW_BASE_WIDTH_BITS: AtomicU64 = AtomicU64::new(640.0f64.to_bits());
static WINDOW_BASE_HEIGHT_BITS: AtomicU64 = AtomicU64::new(620.0f64.to_bits());

const ROTATION_HOST_PADDING_LOGICAL: f64 = 20.0;

fn base_window_logical_size() -> (f64, f64) {
    (
        f64::from_bits(WINDOW_BASE_WIDTH_BITS.load(Ordering::SeqCst)),
        f64::from_bits(WINDOW_BASE_HEIGHT_BITS.load(Ordering::SeqCst)),
    )
}

fn store_base_window_logical_size(width: f64, height: f64) {
    WINDOW_BASE_WIDTH_BITS.store(width.max(1.0).to_bits(), Ordering::SeqCst);
    WINDOW_BASE_HEIGHT_BITS.store(height.max(1.0).to_bits(), Ordering::SeqCst);
}

fn cancel_window_motion() {
    WINDOW_MOTION_GENERATION.fetch_add(1, Ordering::SeqCst);
}

fn current_rotation_angle() -> f64 {
    f64::from_bits(WINDOW_ROTATION_BITS.load(Ordering::SeqCst))
}

fn store_rotation_angle(angle: f64) {
    WINDOW_ROTATION_BITS.store(angle.to_bits(), Ordering::SeqCst);
}

fn current_motion_generation() -> u64 {
    WINDOW_MOTION_GENERATION.load(Ordering::SeqCst)
}

#[tauri::command]
pub(crate) fn hide_main_window(app: AppHandle) {
    cancel_window_motion();
    store_rotation_angle(0.0);
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_ignore_cursor_events(false);
        emit_motion_visual(&window, 0.0, 0.0, 0.0, 0.0, "stop");
        let _ = set_rotation_host_expanded(&window, false);
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
        let _ = window.set_ignore_cursor_events(false);
        emit_motion_visual(&window, 0.0, 0.0, 0.0, 0.0, "stop");
        set_rotation_host_expanded(&window, false)?;
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
        let _ = enabled;
        let _ = window.set_ignore_cursor_events(false);
        emit_motion_visual(&window, 0.0, 0.0, 0.0, 0.0, "stop");
        set_rotation_host_expanded(&window, false)?;
    }

    #[cfg(not(windows))]
    {
        let _ = enabled;
    }

    Ok(())
}

#[tauri::command]
pub(crate) fn freeze_main_window_motion(app: AppHandle) {
    cancel_window_motion();
    #[cfg(windows)]
    if let Some(window) = app.get_webview_window("main") {
        let angle = current_rotation_angle();
        emit_motion_visual(&window, 0.0, 0.0, angle, 0.0, "stop");
        if WINDOW_ROTATION_HOST_EXPANDED.load(Ordering::SeqCst) && distance_to_upright(angle) > 0.08
        {
            spawn_rotated_hit_test(window, current_motion_generation());
        }
    }
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
        let _ = window.set_ignore_cursor_events(false);
        set_rotation_now(&window, 0.0)?;
        set_rotation_host_expanded(&window, false)?;
        emit_motion_visual(&window, 0.0, 0.0, 0.0, 0.0, "stop");
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
    content_width: f64,
    content_height: f64,
    host_expanded: bool,
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

    let (content_width, content_height) = base_window_logical_size();
    let _ = window.emit(
        "yummi://window-motion",
        WindowMotionVisual {
            vx,
            vy,
            speed: vx.hypot(vy),
            angle,
            angular_velocity,
            phase,
            content_width,
            content_height,
            host_expanded: WINDOW_ROTATION_HOST_EXPANDED.load(Ordering::SeqCst),
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
    const ROTATION_PHYSICS_INTERVAL: Duration = Duration::from_micros(8_333);
    const ROTATION_PRESENT_INTERVAL: Duration = Duration::from_micros(16_667);
    const VISUAL_INTERVAL: Duration = Duration::from_millis(16);
    const VELOCITY_SAMPLE_WINDOW: Duration = Duration::from_millis(64);
    const LINEAR_DRAG_PER_SEC: f64 = 2.14;
    const ANGULAR_DRAG_PER_SEC: f64 = 0.91;
    const BOUNCE: f64 = 0.62;
    const WALL_TANGENTIAL_RETENTION: f64 = 0.86;
    const WALL_ANGULAR_RETENTION: f64 = 0.94;
    const STOP_SPEED_PX_S: f64 = 22.0;
    const STOP_ANGULAR_SPEED_DEG_S: f64 = 7.0;
    const MIN_THROW_SPEED_PX_S: f64 = 55.0;
    const MIN_THROW_ANGULAR_SPEED_DEG_S: f64 = 18.0;
    const THROW_SOFT_KNEE_PX_S: f64 = 3600.0;
    const THROW_SOFT_SPAN_PX_S: f64 = 5400.0;
    const MAX_ANGULAR_SPEED_DEG_S: f64 = 1800.0;
    const RELEASE_SPIN_TRANSFER: f64 = 0.72;
    const COLLISION_SPIN_COUPLING: f64 = 0.42;

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
    let grab_x = initial_cursor.x as f64 - window_center_x;
    let grab_y = initial_cursor.y as f64 - window_center_y;
    let (content_width, content_height) = if WINDOW_ROTATION_HOST_EXPANDED.load(Ordering::SeqCst) {
        let (width, height) = base_window_logical_size();
        (width * scale, height * scale)
    } else {
        (initial_size.width as f64, initial_size.height as f64)
    };
    let starting_angle = if free_rotation {
        current_rotation_angle()
    } else {
        0.0
    };

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
            emit_motion_visual(&window, vx, vy, starting_angle, 0.0, "drag");
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
        angular_velocity_from_release(
            grab_x,
            grab_y,
            raw_velocity.0,
            raw_velocity.1,
            content_width,
            content_height,
            RELEASE_SPIN_TRANSFER,
        )
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

    let rotation_motion =
        free_rotation && (angular_velocity != 0.0 || distance_to_upright(starting_angle) > 0.08);
    if set_rotation_host_expanded(&window, rotation_motion).is_err() {
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
        emit_motion_visual(&window, 0.0, 0.0, starting_angle, 0.0, "stop");
        if rotation_motion && distance_to_upright(starting_angle) > 0.08 {
            store_rotation_angle(starting_angle);
            spawn_rotated_hit_test(window.clone(), generation);
        } else if rotation_motion {
            let _ = set_rotation_host_expanded(&window, false);
        }
        return;
    }

    let mut angle = starting_angle;
    store_rotation_angle(angle);
    let mut previous = Instant::now();
    let mut last_window_move = previous - DRAG_INTERVAL;

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

        x += vx * dt;
        y += vy * dt;
        angle += angular_velocity * dt;
        store_rotation_angle(angle);

        let (base_width_logical, base_height_logical) = base_window_logical_size();
        let base_width = base_width_logical * scale;
        let base_height = base_height_logical * scale;
        let (visual_width, visual_height) = rotated_visual_bounds(
            base_width,
            base_height,
            if rotation_motion { angle } else { 0.0 },
        );
        let before_collision_vx = vx;
        let before_collision_vy = vy;
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
        if free_rotation {
            let delta_vx = vx - before_collision_vx;
            let delta_vy = vy - before_collision_vy;
            let hit_vertical_wall = delta_vx.abs() > 0.5;
            let hit_horizontal_wall = delta_vy.abs() > 0.5;
            if hit_vertical_wall || hit_horizontal_wall {
                // A wall impulse reverses the normal component, while contact
                // friction bleeds some tangential velocity and spin. This keeps
                // glancing impacts from looking like perfectly elastic pinball
                // bounces and gives corner hits a more rigid-body feel.
                if hit_vertical_wall {
                    vy *= WALL_TANGENTIAL_RETENTION;
                }
                if hit_horizontal_wall {
                    vx *= WALL_TANGENTIAL_RETENTION;
                }
                angular_velocity *= WALL_ANGULAR_RETENTION;
                angular_velocity +=
                    collision_angular_impulse(base_width, base_height, angle, delta_vx, delta_vy)
                        * COLLISION_SPIN_COUPLING;
                angular_velocity =
                    angular_velocity.clamp(-MAX_ANGULAR_SPEED_DEG_S, MAX_ANGULAR_SPEED_DEG_S);
            }
        }

        let move_interval = if rotation_motion {
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
            let damping = (-LINEAR_DRAG_PER_SEC * dt / strength.max(0.001)).exp();
            vx *= damping;
            vy *= damping;
        }

        if free_rotation {
            angular_velocity *= (-ANGULAR_DRAG_PER_SEC * dt).exp();
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

        let visual_interval = if rotation_motion {
            ROTATION_PRESENT_INTERVAL
        } else {
            VISUAL_INTERVAL
        };
        let has_active_motion = vx != 0.0 || vy != 0.0 || angular_velocity != 0.0;
        if has_active_motion && now.saturating_duration_since(last_visual_emit) >= visual_interval {
            emit_motion_visual(&window, vx, vy, angle, angular_velocity, "glide");
            last_visual_emit = now;
        }

        if !has_active_motion {
            emit_motion_visual(&window, 0.0, 0.0, angle, 0.0, "stop");
            if rotation_motion && distance_to_upright(angle) <= 0.08 {
                let _ = set_rotation_host_expanded(&window, false);
            } else if rotation_motion {
                spawn_rotated_hit_test(window.clone(), generation);
            }
            return;
        }

        let interval = if rotation_motion {
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
fn angular_velocity_from_release(
    grab_x: f64,
    grab_y: f64,
    vx: f64,
    vy: f64,
    width: f64,
    height: f64,
    transfer: f64,
) -> f64 {
    // Thin rectangular plate: I / m = (w² + h²) / 12.
    // The release impulse contributes angular momentum L / m = r × v.
    let inertia_per_mass = (width * width + height * height).max(1.0) / 12.0;
    let omega_rad_s = (grab_x * vy - grab_y * vx) / inertia_per_mass;
    omega_rad_s.to_degrees() * transfer.clamp(0.0, 1.0)
}

#[cfg(windows)]
fn collision_angular_impulse(
    width: f64,
    height: f64,
    angle: f64,
    delta_vx: f64,
    delta_vy: f64,
) -> f64 {
    let impulse = delta_vx.hypot(delta_vy);
    if impulse <= f64::EPSILON {
        return 0.0;
    }

    // The wall impulse acts at the support corner facing the wall.
    let nx = -delta_vx / impulse;
    let ny = -delta_vy / impulse;
    let radians = angle.to_radians();
    let cos = radians.cos();
    let sin = radians.sin();
    let local_nx = cos * nx + sin * ny;
    let local_ny = -sin * nx + cos * ny;
    let local_x = if local_nx >= 0.0 {
        width * 0.5
    } else {
        -width * 0.5
    };
    let local_y = if local_ny >= 0.0 {
        height * 0.5
    } else {
        -height * 0.5
    };
    let contact_x = cos * local_x - sin * local_y;
    let contact_y = sin * local_x + cos * local_y;

    let inertia_per_mass = (width * width + height * height).max(1.0) / 12.0;
    ((contact_x * delta_vy - contact_y * delta_vx) / inertia_per_mass).to_degrees()
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
fn rotated_visual_bounds(width: f64, height: f64, angle: f64) -> (f64, f64) {
    let radians = angle.to_radians();
    let cos = radians.cos().abs();
    let sin = radians.sin().abs();
    (width * cos + height * sin, width * sin + height * cos)
}

#[cfg(windows)]
fn point_inside_rotated_rect(
    cursor_x: i32,
    cursor_y: i32,
    center_x: f64,
    center_y: f64,
    width: f64,
    height: f64,
    angle: f64,
) -> bool {
    let dx = cursor_x as f64 - center_x;
    let dy = cursor_y as f64 - center_y;
    let radians = (-angle).to_radians();
    let local_x = dx * radians.cos() - dy * radians.sin();
    let local_y = dx * radians.sin() + dy * radians.cos();
    local_x.abs() <= width * 0.5 && local_y.abs() <= height * 0.5
}

#[cfg(windows)]
fn spawn_rotated_hit_test(window: tauri::WebviewWindow, generation: u64) {
    let _ = std::thread::Builder::new()
        .name("yummi-window-hit-test".into())
        .spawn(move || {
            use std::{thread, time::Duration};

            let Ok(position) = window.outer_position() else {
                return;
            };
            let Ok(size) = window.outer_size() else {
                return;
            };
            let scale = window.scale_factor().unwrap_or(1.0).max(0.1);
            let (base_width, base_height) = base_window_logical_size();
            let center_x = position.x as f64 + size.width as f64 * 0.5;
            let center_y = position.y as f64 + size.height as f64 * 0.5;
            let content_width = base_width * scale;
            let content_height = base_height * scale;
            let angle = current_rotation_angle();
            let mut last_ignore = None;

            while motion_is_current(generation)
                && WINDOW_ROTATION_HOST_EXPANDED.load(Ordering::SeqCst)
                && distance_to_upright(current_rotation_angle()) > 0.08
            {
                let Some(cursor) = cursor_position() else {
                    break;
                };
                let ignore = !point_inside_rotated_rect(
                    cursor.x,
                    cursor.y,
                    center_x,
                    center_y,
                    content_width,
                    content_height,
                    angle,
                );
                if last_ignore != Some(ignore) {
                    let _ = window.set_ignore_cursor_events(ignore);
                    last_ignore = Some(ignore);
                }
                thread::sleep(Duration::from_millis(10));
            }
            let _ = window.set_ignore_cursor_events(false);
        });
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
    emit_motion_visual(window, 0.0, 0.0, angle, 0.0, "stop");
    Ok(())
}

#[cfg(windows)]
fn set_rotation_host_expanded(window: &tauri::WebviewWindow, expanded: bool) -> Result<(), String> {
    use windows::Win32::{
        Foundation::HWND,
        UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOSENDCHANGING, SWP_NOZORDER},
    };

    let currently_expanded = WINDOW_ROTATION_HOST_EXPANDED.load(Ordering::SeqCst);
    if currently_expanded == expanded {
        return Ok(());
    }

    let old_position = window.outer_position().map_err(|error| error.to_string())?;
    let old_size = window.outer_size().map_err(|error| error.to_string())?;
    let scale = window.scale_factor().unwrap_or(1.0).max(0.1);

    let (target_width_logical, target_height_logical) = if expanded {
        let base_width = old_size.width as f64 / scale;
        let base_height = old_size.height as f64 / scale;
        store_base_window_logical_size(base_width, base_height);
        let side = base_width.hypot(base_height) + ROTATION_HOST_PADDING_LOGICAL;

        // Freeze the WebView content at the current user-selected size before
        // enlarging the transparent host, avoiding a one-frame content stretch.
        WINDOW_ROTATION_HOST_EXPANDED.store(true, Ordering::SeqCst);
        emit_motion_visual(window, 0.0, 0.0, current_rotation_angle(), 0.0, "stop");
        std::thread::sleep(std::time::Duration::from_millis(1));
        (side, side)
    } else {
        base_window_logical_size()
    };

    let target_physical_width = (target_width_logical * scale).round().max(1.0) as i32;
    let target_physical_height = (target_height_logical * scale).round().max(1.0) as i32;
    let center_x = old_position.x as f64 + old_size.width as f64 * 0.5;
    let center_y = old_position.y as f64 + old_size.height as f64 * 0.5;
    let x = (center_x - target_physical_width as f64 * 0.5).round() as i32;
    let y = (center_y - target_physical_height as f64 * 0.5).round() as i32;
    let native = window.hwnd().map_err(|error| error.to_string())?;

    if let Err(error) = unsafe {
        SetWindowPos(
            HWND(native.0),
            None,
            x,
            y,
            target_physical_width,
            target_physical_height,
            SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOSENDCHANGING,
        )
    } {
        if expanded {
            WINDOW_ROTATION_HOST_EXPANDED.store(false, Ordering::SeqCst);
            emit_motion_visual(window, 0.0, 0.0, current_rotation_angle(), 0.0, "stop");
        }
        return Err(error.to_string());
    }

    WINDOW_ROTATION_HOST_EXPANDED.store(expanded, Ordering::SeqCst);
    if !expanded {
        emit_motion_visual(window, 0.0, 0.0, current_rotation_angle(), 0.0, "stop");
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
        angular_velocity_from_release, nearest_upright_angle, release_velocity,
        rotated_visual_bounds, soften_throw_velocity,
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
    fn rotated_bounds_match_rectangle_geometry() {
        let (width, height) = rotated_visual_bounds(640.0, 620.0, 45.0);
        assert!(width > 890.0 && width < 892.0);
        assert!(height > 890.0 && height < 892.0);
    }

    #[test]
    fn release_spin_uses_grab_offset_and_moment_of_inertia() {
        let centered = angular_velocity_from_release(0.0, 0.0, 0.0, 2_000.0, 640.0, 620.0, 0.72);
        let edge = angular_velocity_from_release(300.0, 0.0, 0.0, 2_000.0, 640.0, 620.0, 0.72);
        assert!(centered.abs() < 0.001);
        assert!(edge > 300.0);
    }
}
