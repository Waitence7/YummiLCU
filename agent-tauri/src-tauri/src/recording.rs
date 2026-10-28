use serde::Serialize;

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecordingStatus {
    pub supported: bool,
    pub recording: bool,
    pub clip_buffer: bool,
    pub recording_path: Option<String>,
    pub buffered_seconds: u64,
    pub last_clip_path: Option<String>,
    pub last_error: Option<String>,
    pub video_only: bool,
    pub recording_source: String,
    pub clip_source: String,
}

#[cfg(windows)]
mod imp {
    use std::{
        collections::{HashSet, VecDeque},
        fs,
        path::{Path, PathBuf},
        sync::{Mutex, MutexGuard, OnceLock},
        time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    };

    use windows::{
        core::HSTRING,
        Foundation::TimeSpan,
        Media::{
            Editing::{MediaClip, MediaComposition},
            Transcoding::TranscodeFailureReason,
        },
        Storage::StorageFile,
        Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
    };
    use windows_capture::{
        capture::{CaptureControl, Context, GraphicsCaptureApiHandler},
        encoder::{
            AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder,
            VideoSettingsSubType,
        },
        frame::Frame,
        graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl},
        monitor::Monitor,
        settings::{
            ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
            MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
        },
        window::Window,
    };

    use super::RecordingStatus;

    const VIDEO_BITRATE: u32 = 15_000_000;
    const VIDEO_FPS: u32 = 60;
    const SEGMENT_DURATION: Duration = Duration::from_secs(10);
    const MAX_BUFFER_DURATION: Duration = Duration::from_secs(75);
    const MIN_SEGMENT_DURATION: Duration = Duration::from_millis(300);
    const LEAGUE_WINDOW_TITLES: [&str; 2] = ["League of Legends (TM) Client", "League of Legends"];

    type CaptureError = String;

    #[derive(Clone)]
    struct EncoderFlags {
        output_path: PathBuf,
        width: u32,
        height: u32,
    }

    fn new_encoder(path: &Path, width: u32, height: u32) -> Result<VideoEncoder, CaptureError> {
        VideoEncoder::new(
            VideoSettingsBuilder::new(width, height)
                .sub_type(VideoSettingsSubType::H264)
                .bitrate(VIDEO_BITRATE)
                .frame_rate(VIDEO_FPS),
            AudioSettingsBuilder::new().disabled(true),
            ContainerSettingsBuilder::new(),
            path,
        )
        .map_err(|error| format!("영상 인코더 시작 실패: {error}"))
    }

    struct GeneralCapture {
        encoder: Option<VideoEncoder>,
    }

    impl GraphicsCaptureApiHandler for GeneralCapture {
        type Flags = EncoderFlags;
        type Error = CaptureError;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            Ok(Self {
                encoder: Some(new_encoder(
                    &ctx.flags.output_path,
                    ctx.flags.width,
                    ctx.flags.height,
                )?),
            })
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            _capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            if let Some(encoder) = self.encoder.as_mut() {
                encoder
                    .send_frame(frame)
                    .map_err(|error| format!("녹화 프레임 인코딩 실패: {error}"))?;
            }
            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            if let Some(encoder) = self.encoder.take() {
                encoder
                    .finish()
                    .map_err(|error| format!("녹화 파일 마무리 실패: {error}"))?;
            }
            Ok(())
        }
    }

    #[derive(Clone, Debug)]
    struct Segment {
        path: PathBuf,
        duration: Duration,
    }

    #[derive(Clone)]
    struct RollingFlags {
        directory: PathBuf,
        width: u32,
        height: u32,
    }

    struct RollingCapture {
        directory: PathBuf,
        width: u32,
        height: u32,
        encoder: Option<VideoEncoder>,
        current_path: PathBuf,
        current_started: Instant,
        next_index: u64,
        segments: VecDeque<Segment>,
        protected: HashSet<PathBuf>,
    }

    impl RollingCapture {
        fn segment_path(&self, index: u64) -> PathBuf {
            self.directory.join(format!("segment-{index:06}.mp4"))
        }

        fn start_segment(&mut self) -> Result<(), CaptureError> {
            let path = self.segment_path(self.next_index);
            self.next_index = self.next_index.saturating_add(1);
            self.encoder = Some(new_encoder(&path, self.width, self.height)?);
            self.current_path = path;
            self.current_started = Instant::now();
            Ok(())
        }

        fn commit_current_segment(&mut self) -> Result<(), CaptureError> {
            let duration = self.current_started.elapsed();
            if let Some(encoder) = self.encoder.take() {
                encoder
                    .finish()
                    .map_err(|error| format!("클립 버퍼 세그먼트 마무리 실패: {error}"))?;
            }
            if duration >= MIN_SEGMENT_DURATION
                && fs::metadata(&self.current_path).is_ok_and(|metadata| metadata.len() > 0)
            {
                self.segments.push_back(Segment {
                    path: self.current_path.clone(),
                    duration,
                });
            } else if !self.current_path.as_os_str().is_empty() {
                let _ = fs::remove_file(&self.current_path);
            }
            Ok(())
        }

        fn rotate_segment(&mut self) -> Result<(), CaptureError> {
            self.commit_current_segment()?;
            self.start_segment()?;
            self.prune();
            Ok(())
        }

        fn buffered_duration(&self) -> Duration {
            let completed = self.segments.iter().fold(Duration::ZERO, |sum, segment| {
                sum.saturating_add(segment.duration)
            });
            let current = if self.encoder.is_some() {
                self.current_started.elapsed()
            } else {
                Duration::ZERO
            };
            completed.saturating_add(current).min(MAX_BUFFER_DURATION)
        }

        fn reserve_export(
            &mut self,
            requested: Duration,
        ) -> Result<ExportReservation, CaptureError> {
            if self.encoder.is_some() && self.current_started.elapsed() >= MIN_SEGMENT_DURATION {
                self.rotate_segment()?;
            }
            if self.segments.is_empty() {
                return Err("아직 저장할 클립 영상이 없습니다.".into());
            }

            let mut selected = Vec::new();
            let mut total = Duration::ZERO;
            for segment in self.segments.iter().rev() {
                selected.push(segment.path.clone());
                total = total.saturating_add(segment.duration);
                if total >= requested {
                    break;
                }
            }
            selected.reverse();
            for path in &selected {
                self.protected.insert(path.clone());
            }
            Ok(ExportReservation {
                paths: selected,
                requested: requested.min(total),
            })
        }

        fn release_export(&mut self, paths: &[PathBuf]) {
            for path in paths {
                self.protected.remove(path);
            }
            self.prune();
        }

        fn prune(&mut self) {
            let mut total = self.segments.iter().fold(Duration::ZERO, |sum, segment| {
                sum.saturating_add(segment.duration)
            });
            while total > MAX_BUFFER_DURATION {
                let Some(front) = self.segments.front() else {
                    break;
                };
                if self.protected.contains(&front.path) {
                    break;
                }
                let segment = self
                    .segments
                    .pop_front()
                    .expect("front segment disappeared");
                total = total.saturating_sub(segment.duration);
                let _ = fs::remove_file(segment.path);
            }
        }
    }

    impl GraphicsCaptureApiHandler for RollingCapture {
        type Flags = RollingFlags;
        type Error = CaptureError;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            fs::create_dir_all(&ctx.flags.directory)
                .map_err(|error| format!("클립 버퍼 폴더 생성 실패: {error}"))?;
            let mut capture = Self {
                directory: ctx.flags.directory,
                width: ctx.flags.width,
                height: ctx.flags.height,
                encoder: None,
                current_path: PathBuf::new(),
                current_started: Instant::now(),
                next_index: 0,
                segments: VecDeque::new(),
                protected: HashSet::new(),
            };
            capture.start_segment()?;
            Ok(capture)
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            _capture_control: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            if self.current_started.elapsed() >= SEGMENT_DURATION {
                self.rotate_segment()?;
            }
            if let Some(encoder) = self.encoder.as_mut() {
                encoder
                    .send_frame(frame)
                    .map_err(|error| format!("클립 버퍼 프레임 인코딩 실패: {error}"))?;
            }
            Ok(())
        }

        fn on_closed(&mut self) -> Result<(), Self::Error> {
            self.commit_current_segment()
        }
    }

    struct ExportReservation {
        paths: Vec<PathBuf>,
        requested: Duration,
    }

    struct RecordingManager {
        general: Option<CaptureControl<GeneralCapture, CaptureError>>,
        rolling: Option<CaptureControl<RollingCapture, CaptureError>>,
        recording_path: Option<PathBuf>,
        rolling_directory: Option<PathBuf>,
        last_clip_path: Option<PathBuf>,
        last_error: Option<String>,
        clip_exporting: bool,
        recording_source: String,
        clip_source: String,
    }

    impl RecordingManager {
        fn new() -> Self {
            Self {
                general: None,
                rolling: None,
                recording_path: None,
                rolling_directory: None,
                last_clip_path: None,
                last_error: None,
                clip_exporting: false,
                recording_source: "monitor".into(),
                clip_source: "leagueWindow".into(),
            }
        }

        fn cleanup_finished(&mut self) {
            if self
                .general
                .as_ref()
                .is_some_and(CaptureControl::is_finished)
            {
                if let Some(control) = self.general.take() {
                    if let Err(error) = control.wait() {
                        self.last_error = Some(format!("녹화 세션 종료 오류: {error}"));
                    }
                }
            }
            if self
                .rolling
                .as_ref()
                .is_some_and(CaptureControl::is_finished)
            {
                if let Some(control) = self.rolling.take() {
                    if let Err(error) = control.wait() {
                        self.last_error = Some(format!("클립 버퍼 종료 오류: {error}"));
                    }
                }
            }
        }

        fn status(&mut self) -> RecordingStatus {
            self.cleanup_finished();
            let buffered_seconds = self
                .rolling
                .as_ref()
                .map(|control| control.callback().lock().buffered_duration().as_secs())
                .unwrap_or(0);
            RecordingStatus {
                supported: is_capture_supported(),
                recording: self.general.is_some(),
                clip_buffer: self.rolling.is_some(),
                recording_path: self.recording_path.as_ref().map(path_string),
                buffered_seconds,
                last_clip_path: self.last_clip_path.as_ref().map(path_string),
                last_error: self.last_error.clone(),
                video_only: true,
                recording_source: self.recording_source.clone(),
                clip_source: self.clip_source.clone(),
            }
        }
    }

    static MANAGER: OnceLock<Mutex<RecordingManager>> = OnceLock::new();

    fn manager() -> &'static Mutex<RecordingManager> {
        MANAGER.get_or_init(|| Mutex::new(RecordingManager::new()))
    }

    fn lock_manager() -> Result<MutexGuard<'static, RecordingManager>, String> {
        manager()
            .lock()
            .map_err(|_| "녹화 상태 잠금이 손상되었습니다.".to_string())
    }

    fn is_capture_supported() -> bool {
        let Ok(_winrt) = WinRtGuard::new() else {
            return false;
        };
        GraphicsCaptureApi::is_supported().unwrap_or(false)
    }

    fn ensure_capture_supported() -> Result<(), String> {
        if is_capture_supported() {
            Ok(())
        } else {
            Err("이 Windows 버전에서는 Windows Graphics Capture를 사용할 수 없습니다. Windows 10 1903 이상이 필요합니다.".into())
        }
    }

    fn timestamp_ms() -> u128 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    }

    fn path_string(path: &PathBuf) -> String {
        path.to_string_lossy().into_owned()
    }

    fn video_root() -> PathBuf {
        dirs::video_dir()
            .or_else(|| dirs::home_dir().map(|home| home.join("Videos")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Yummi")
    }

    fn recording_directory() -> PathBuf {
        video_root().join("Recordings")
    }

    fn clip_directory() -> PathBuf {
        video_root().join("Clips")
    }

    fn rolling_root() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(std::env::temp_dir)
            .join("Yummi")
            .join("recording-buffer")
    }

    fn league_window() -> Result<Window, String> {
        let windows =
            Window::enumerate().map_err(|error| format!("Windows 창 목록 조회 실패: {error}"))?;
        for preferred in LEAGUE_WINDOW_TITLES {
            if let Some(window) = windows.iter().find(|window| {
                window
                    .title()
                    .is_ok_and(|title| title.eq_ignore_ascii_case(preferred))
            }) {
                return Ok(*window);
            }
        }
        windows
            .into_iter()
            .find(|window| {
                window.title().is_ok_and(|title| {
                    let title = title.to_ascii_lowercase();
                    title.contains("league of legends") && !title.contains("client")
                })
            })
            .ok_or_else(|| {
                "League of Legends 게임 창을 찾지 못했습니다. 게임에 진입한 뒤 다시 시도해 주세요."
                    .into()
            })
    }

    fn monitor_for_general_recording() -> Result<Monitor, String> {
        if let Ok(window) = league_window() {
            if let Some(monitor) = window.monitor() {
                return Ok(monitor);
            }
        }
        Monitor::primary().map_err(|error| format!("주 모니터를 찾지 못했습니다: {error}"))
    }

    fn general_monitor_settings(
        monitor: Monitor,
        output_path: PathBuf,
    ) -> Result<Settings<EncoderFlags, Monitor>, String> {
        let width = monitor
            .width()
            .map_err(|error| format!("모니터 너비 조회 실패: {error}"))?;
        let height = monitor
            .height()
            .map_err(|error| format!("모니터 높이 조회 실패: {error}"))?;
        Ok(Settings::new(
            monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            EncoderFlags {
                output_path,
                width,
                height,
            },
        ))
    }

    fn rolling_window_settings(
        window: Window,
        directory: PathBuf,
    ) -> Result<Settings<RollingFlags, Window>, String> {
        let width = u32::try_from(
            window
                .width()
                .map_err(|error| format!("게임 창 너비 조회 실패: {error}"))?,
        )
        .map_err(|_| "게임 창 너비가 올바르지 않습니다.".to_string())?;
        let height = u32::try_from(
            window
                .height()
                .map_err(|error| format!("게임 창 높이 조회 실패: {error}"))?,
        )
        .map_err(|_| "게임 창 높이가 올바르지 않습니다.".to_string())?;
        Ok(Settings::new(
            window,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            RollingFlags {
                directory,
                width,
                height,
            },
        ))
    }

    fn general_window_settings(
        window: Window,
        output_path: PathBuf,
    ) -> Result<Settings<EncoderFlags, Window>, String> {
        let width = u32::try_from(
            window
                .width()
                .map_err(|error| format!("게임 창 너비 조회 실패: {error}"))?,
        )
        .map_err(|_| "게임 창 너비가 올바르지 않습니다.".to_string())?;
        let height = u32::try_from(
            window
                .height()
                .map_err(|error| format!("게임 창 높이 조회 실패: {error}"))?,
        )
        .map_err(|_| "게임 창 높이가 올바르지 않습니다.".to_string())?;
        Ok(Settings::new(
            window,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            EncoderFlags {
                output_path,
                width,
                height,
            },
        ))
    }

    fn rolling_monitor_settings(
        monitor: Monitor,
        directory: PathBuf,
    ) -> Result<Settings<RollingFlags, Monitor>, String> {
        let width = monitor
            .width()
            .map_err(|error| format!("모니터 너비 조회 실패: {error}"))?;
        let height = monitor
            .height()
            .map_err(|error| format!("모니터 높이 조회 실패: {error}"))?;
        Ok(Settings::new(
            monitor,
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            RollingFlags {
                directory,
                width,
                height,
            },
        ))
    }

    fn normalized_source(source: Option<&str>, default: &str) -> Result<&'static str, String> {
        match source.unwrap_or(default) {
            "monitor" => Ok("monitor"),
            "leagueWindow" => Ok("leagueWindow"),
            _ => Err("캡처 대상은 monitor 또는 leagueWindow 여야 합니다.".into()),
        }
    }

    pub(super) fn status() -> Result<RecordingStatus, String> {
        Ok(lock_manager()?.status())
    }

    pub(super) fn start_recording(source: Option<&str>) -> Result<RecordingStatus, String> {
        ensure_capture_supported()?;
        let source = normalized_source(source, "monitor")?;
        let mut manager = lock_manager()?;
        manager.cleanup_finished();
        if manager.general.is_some() {
            return Ok(manager.status());
        }

        let directory = recording_directory();
        fs::create_dir_all(&directory).map_err(|error| format!("녹화 폴더 생성 실패: {error}"))?;
        let path = directory.join(format!("Yummi-recording-{}.mp4", timestamp_ms()));
        let control = if source == "leagueWindow" {
            let settings = general_window_settings(league_window()?, path.clone())?;
            GeneralCapture::start_free_threaded(settings)
                .map_err(|error| format!("일반 녹화 시작 실패: {error}"))?
        } else {
            let settings =
                general_monitor_settings(monitor_for_general_recording()?, path.clone())?;
            GeneralCapture::start_free_threaded(settings)
                .map_err(|error| format!("일반 녹화 시작 실패: {error}"))?
        };

        manager.general = Some(control);
        manager.recording_path = Some(path);
        manager.recording_source = source.into();
        manager.last_error = None;
        Ok(manager.status())
    }

    pub(super) fn stop_recording() -> Result<RecordingStatus, String> {
        let control = {
            let mut manager = lock_manager()?;
            manager.cleanup_finished();
            manager.general.take()
        };
        if let Some(control) = control {
            control
                .stop()
                .map_err(|error| format!("일반 녹화 중지 실패: {error}"))?;
        }
        Ok(lock_manager()?.status())
    }

    pub(super) fn start_clip_buffer(source: Option<&str>) -> Result<RecordingStatus, String> {
        ensure_capture_supported()?;
        let source = normalized_source(source, "leagueWindow")?;
        let mut manager = lock_manager()?;
        manager.cleanup_finished();
        if manager.rolling.is_some() {
            return Ok(manager.status());
        }

        let root = rolling_root();
        fs::create_dir_all(&root).map_err(|error| format!("클립 버퍼 루트 생성 실패: {error}"))?;
        let directory = root.join(format!("session-{}", timestamp_ms()));
        fs::create_dir_all(&directory).map_err(|error| format!("클립 버퍼 생성 실패: {error}"))?;
        let control = if source == "monitor" {
            let settings =
                rolling_monitor_settings(monitor_for_general_recording()?, directory.clone())?;
            RollingCapture::start_free_threaded(settings)
                .map_err(|error| format!("클립 버퍼 시작 실패: {error}"))?
        } else {
            let settings = rolling_window_settings(league_window()?, directory.clone())?;
            RollingCapture::start_free_threaded(settings)
                .map_err(|error| format!("클립 버퍼 시작 실패: {error}"))?
        };

        manager.rolling = Some(control);
        manager.rolling_directory = Some(directory);
        manager.clip_source = source.into();
        manager.last_error = None;
        Ok(manager.status())
    }

    pub(super) fn stop_clip_buffer() -> Result<RecordingStatus, String> {
        let (control, directory) = {
            let mut manager = lock_manager()?;
            manager.cleanup_finished();
            if manager.clip_exporting {
                return Err("클립 저장이 진행 중이라 버퍼를 중지할 수 없습니다.".into());
            }
            (manager.rolling.take(), manager.rolling_directory.take())
        };
        if let Some(control) = control {
            control
                .stop()
                .map_err(|error| format!("클립 버퍼 중지 실패: {error}"))?;
        }
        if let Some(directory) = directory {
            let _ = fs::remove_dir_all(directory);
        }
        Ok(lock_manager()?.status())
    }

    pub(super) fn save_last_clip(seconds: u64) -> Result<RecordingStatus, String> {
        if !(5..=60).contains(&seconds) {
            return Err("클립 길이는 5~60초 사이여야 합니다.".into());
        }

        let reservation = {
            let mut manager = lock_manager()?;
            manager.cleanup_finished();
            if manager.clip_exporting {
                return Err("이미 다른 클립을 저장하고 있습니다.".into());
            }
            let control = manager
                .rolling
                .as_ref()
                .ok_or_else(|| "클립 버퍼가 실행 중이 아닙니다.".to_string())?;
            let reservation = control
                .callback()
                .lock()
                .reserve_export(Duration::from_secs(seconds))?;
            manager.clip_exporting = true;
            reservation
        };

        fs::create_dir_all(clip_directory())
            .map_err(|error| format!("클립 폴더 생성 실패: {error}"))?;
        let output = clip_directory().join(format!("Yummi-clip-{seconds}s-{}.mp4", timestamp_ms()));
        let export_result = render_clip(&reservation.paths, reservation.requested, &output);

        let mut manager = lock_manager()?;
        manager.clip_exporting = false;
        if let Some(control) = manager.rolling.as_ref() {
            control.callback().lock().release_export(&reservation.paths);
        }
        match export_result {
            Ok(()) => {
                manager.last_clip_path = Some(output);
                manager.last_error = None;
            }
            Err(error) => {
                manager.last_error = Some(error.clone());
                let _ = fs::remove_file(&output);
                return Err(error);
            }
        }
        Ok(manager.status())
    }

    struct WinRtGuard;

    impl WinRtGuard {
        fn new() -> Result<Self, String> {
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }
                .map_err(|error| format!("WinRT 초기화 실패: {error}"))?;
            Ok(Self)
        }
    }

    impl Drop for WinRtGuard {
        fn drop(&mut self) {
            unsafe { RoUninitialize() };
        }
    }

    fn storage_file(path: &Path) -> Result<StorageFile, String> {
        let absolute =
            fs::canonicalize(path).map_err(|error| format!("영상 파일 경로 확인 실패: {error}"))?;
        StorageFile::GetFileFromPathAsync(&HSTRING::from(absolute.as_os_str()))
            .map_err(|error| format!("영상 파일 열기 실패: {error}"))?
            .get()
            .map_err(|error| format!("영상 파일 열기 실패: {error}"))
    }

    fn render_clip(paths: &[PathBuf], requested: Duration, output: &Path) -> Result<(), String> {
        if paths.is_empty() {
            return Err("클립으로 저장할 세그먼트가 없습니다.".into());
        }
        let _winrt = WinRtGuard::new()?;
        let composition =
            MediaComposition::new().map_err(|error| format!("클립 합성기 생성 실패: {error}"))?;
        let clips = composition
            .Clips()
            .map_err(|error| format!("클립 목록 생성 실패: {error}"))?;

        let mut total_ticks: i64 = 0;
        let mut created = Vec::with_capacity(paths.len());
        for path in paths {
            let file = storage_file(path)?;
            let clip = MediaClip::CreateFromFileAsync(&file)
                .map_err(|error| format!("클립 세그먼트 로드 실패: {error}"))?
                .get()
                .map_err(|error| format!("클립 세그먼트 로드 실패: {error}"))?;
            let duration = clip
                .OriginalDuration()
                .map_err(|error| format!("클립 길이 조회 실패: {error}"))?;
            total_ticks = total_ticks.saturating_add(duration.Duration.max(0));
            created.push(clip);
        }

        let requested_ticks = i64::try_from(requested.as_nanos() / 100)
            .unwrap_or(i64::MAX)
            .max(1);
        if total_ticks > requested_ticks {
            if let Some(first) = created.first() {
                first
                    .SetTrimTimeFromStart(TimeSpan {
                        Duration: total_ticks - requested_ticks,
                    })
                    .map_err(|error| format!("클립 시작점 자르기 실패: {error}"))?;
            }
        }
        for clip in created {
            clips
                .Append(&clip)
                .map_err(|error| format!("클립 합성 목록 추가 실패: {error}"))?;
        }

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("클립 저장 폴더 생성 실패: {error}"))?;
        }
        fs::File::create(output).map_err(|error| format!("클립 출력 파일 생성 실패: {error}"))?;
        let destination = storage_file(output)?;
        let result = composition
            .RenderToFileAsync(&destination)
            .map_err(|error| format!("클립 렌더 시작 실패: {error}"))?
            .get()
            .map_err(|error| format!("클립 렌더 실패: {error}"))?;
        if result != TranscodeFailureReason::None {
            return Err(format!("클립 렌더 실패: {result:?}"));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod imp {
    use super::RecordingStatus;

    fn unsupported() -> String {
        "녹화 기능은 Windows 10 1903 이상에서 지원됩니다.".into()
    }

    pub(super) fn status() -> Result<RecordingStatus, String> {
        Ok(RecordingStatus {
            supported: false,
            video_only: true,
            recording_source: "monitor".into(),
            clip_source: "leagueWindow".into(),
            ..RecordingStatus::default()
        })
    }

    pub(super) fn start_recording(_source: Option<&str>) -> Result<RecordingStatus, String> {
        Err(unsupported())
    }
    pub(super) fn stop_recording() -> Result<RecordingStatus, String> {
        Err(unsupported())
    }
    pub(super) fn start_clip_buffer(_source: Option<&str>) -> Result<RecordingStatus, String> {
        Err(unsupported())
    }
    pub(super) fn stop_clip_buffer() -> Result<RecordingStatus, String> {
        Err(unsupported())
    }
    pub(super) fn save_last_clip(_seconds: u64) -> Result<RecordingStatus, String> {
        Err(unsupported())
    }
}

pub(crate) fn status() -> Result<RecordingStatus, String> {
    imp::status()
}
pub(crate) fn start_recording(source: Option<&str>) -> Result<RecordingStatus, String> {
    imp::start_recording(source)
}
pub(crate) fn stop_recording() -> Result<RecordingStatus, String> {
    imp::stop_recording()
}
pub(crate) fn start_clip_buffer(source: Option<&str>) -> Result<RecordingStatus, String> {
    imp::start_clip_buffer(source)
}
pub(crate) fn stop_clip_buffer() -> Result<RecordingStatus, String> {
    imp::stop_clip_buffer()
}
pub(crate) fn save_last_clip(seconds: u64) -> Result<RecordingStatus, String> {
    imp::save_last_clip(seconds)
}
