use ruzstd::decoding::StreamingDecoder;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs::{self, File},
    io::{Cursor, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        OnceLock, RwLock,
    },
    time::UNIX_EPOCH,
};
use xxhash_rust::xxh64::xxh64;

const WAD_REL: &str = "Game/DATA/FINAL/Maps/Shipping/Common.wad.client";
const EVENTS_BANK: &str = "assets/sounds/wwise2016/sfx/shared/hud_global_events.bnk";
const AUDIO_BANK: &str = "assets/sounds/wwise2016/sfx/shared/hud_global_audio.bnk";
const VGMSTREAM_URL: &str =
    "https://github.com/vgmstream/vgmstream/releases/download/r2168/vgmstream-win64.zip";
const VGMSTREAM_SHA256: &str = "692d45070852a35c1fa42726aa3c9de528e44eec925d37d99b1074256b99a229";
const MAX_TOOL_ZIP_BYTES: usize = 16 * 1024 * 1024;
const PING_EVENTS: [(&str, &str); 8] = [
    ("retreat", "Play_sfx_hud_base_Pings_SRP_12"),
    ("push", "Play_sfx_hud_base_Pings_SRP_9"),
    ("on_my_way", "Play_sfx_hud_base_Pings_OnMyWay"),
    ("all_in", "Play_sfx_hud_base_Pings_SRP_4"),
    ("assist_me", "Play_sfx_hud_base_Pings_ComeHere"),
    ("need_vision", "Play_sfx_hud_base_Pings_SRP_7"),
    ("enemy_missing", "Play_sfx_hud_base_Pings_MIA"),
    ("enemy_vision", "Play_sfx_hud_base_Pings_AreaIsWarded"),
];

static READY: OnceLock<RwLock<HashMap<String, PathBuf>>> = OnceLock::new();
static PREPARING: AtomicBool = AtomicBool::new(false);
fn ready_slot() -> &'static RwLock<HashMap<String, PathBuf>> {
    READY.get_or_init(|| RwLock::new(HashMap::new()))
}

pub(crate) fn warm_up() {
    #[cfg(not(windows))]
    return;
    #[cfg(windows)]
    {
        if ready_slot()
            .read()
            .is_ok_and(|v| v.len() >= PING_EVENTS.len())
        {
            return;
        }
        if PREPARING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        tauri::async_runtime::spawn(async {
            match prepare_audio().await {
                Ok(paths) => {
                    if let Ok(mut current) = ready_slot().write() {
                        *current = paths;
                    }
                }
                Err(error) => eprintln!("League ping audio prepare failed: {error}"),
            }
            PREPARING.store(false, Ordering::Release);
        });
    }
}

pub(crate) fn play(ping_type: &str) -> bool {
    #[cfg(not(windows))]
    {
        let _ = ping_type;
        false
    }
    #[cfg(windows)]
    {
        let path = ready_slot()
            .read()
            .ok()
            .and_then(|v| v.get(ping_type).cloned());
        let Some(path) = path else {
            warm_up();
            return false;
        };
        play_wav(&path)
    }
}

#[cfg(windows)]
fn play_wav(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use windows::{
        core::PCWSTR,
        Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME, SND_NODEFAULT},
    };
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    unsafe {
        PlaySoundW(
            PCWSTR(wide.as_ptr()),
            None,
            SND_ASYNC | SND_FILENAME | SND_NODEFAULT,
        )
    }
    .as_bool()
}

#[cfg(windows)]
async fn prepare_audio() -> Result<HashMap<String, PathBuf>, String> {
    let league_root =
        find_league_root().ok_or_else(|| "League 설치 경로를 찾지 못했습니다.".to_owned())?;
    let wad = league_root.join(WAD_REL);
    let metadata = fs::metadata(&wad).map_err(|e| format!("Common.wad.client 확인 실패: {e}"))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|v| v.duration_since(UNIX_EPOCH).ok())
        .map(|v| v.as_secs())
        .unwrap_or(0);
    let cache = cache_root().join(format!("{}-{}", metadata.len(), modified));
    fs::create_dir_all(&cache).map_err(|e| format!("핑 음원 캐시 생성 실패: {e}"))?;
    let cached = cached_outputs(&cache);
    if cached.len() == PING_EVENTS.len() {
        return Ok(cached);
    }
    let vgmstream = ensure_vgmstream().await?;
    tokio::task::spawn_blocking(move || extract_ping_audio(&wad, &cache, &vgmstream))
        .await
        .map_err(|e| format!("핑 음원 추출 worker 실패: {e}"))?
}

fn cache_root() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("YummiLabs")
        .join("YummiLcu")
        .join("league-ping-audio-v1")
}
fn cached_outputs(dir: &Path) -> HashMap<String, PathBuf> {
    PING_EVENTS
        .iter()
        .filter_map(|(kind, _)| {
            let path = dir.join(format!("{kind}.wav"));
            fs::metadata(&path)
                .ok()
                .filter(|m| m.len() > 44)
                .map(|_| ((*kind).to_owned(), path))
        })
        .collect()
}

fn find_league_root() -> Option<PathBuf> {
    let valid = |root: PathBuf| root.join(WAD_REL).is_file().then_some(root);
    if let Some(path) = std::env::var_os("YUMMI_LEAGUE_PATH").map(PathBuf::from) {
        if let Some(root) = valid(path) {
            return Some(root);
        }
    }
    if let Some(program_data) = std::env::var_os("ProgramData") {
        let metadata = PathBuf::from(program_data)
            .join("Riot Games")
            .join("Metadata")
            .join("league_of_legends.live")
            .join("league_of_legends.live.product_settings.yaml");
        if let Ok(contents) = fs::read_to_string(metadata) {
            for line in contents.lines() {
                let Some(value) = line.trim().strip_prefix("product_install_full_path:") else {
                    continue;
                };
                let value = value.trim().trim_matches(['\'', '"']);
                if !value.is_empty() {
                    if let Some(root) = valid(PathBuf::from(value)) {
                        return Some(root);
                    }
                }
            }
        }
    }
    for drive in b'C'..=b'Z' {
        let drive = drive as char;
        for path in [
            format!(r"{drive}:\Riot Games\League of Legends"),
            format!(r"{drive}:\Program Files\Riot Games\League of Legends"),
            format!(r"{drive}:\League of Legends"),
        ] {
            if let Some(root) = valid(PathBuf::from(path)) {
                return Some(root);
            }
        }
    }
    None
}

#[cfg(windows)]
async fn ensure_vgmstream() -> Result<PathBuf, String> {
    let tool_dir = cache_root().join("tools").join("vgmstream-r2168");
    let exe = tool_dir.join("vgmstream-cli.exe");
    if exe.is_file() {
        return Ok(exe);
    }
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .user_agent("Yummi-LCU-Agent-Ping-Audio")
        .build()
        .map_err(|e| format!("vgmstream 다운로드 클라이언트 생성 실패: {e}"))?;
    let response = client
        .get(VGMSTREAM_URL)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|e| format!("vgmstream 다운로드 실패: {e}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("vgmstream 다운로드 읽기 실패: {e}"))?;
    if bytes.is_empty() || bytes.len() > MAX_TOOL_ZIP_BYTES {
        return Err("vgmstream 다운로드 크기 오류".into());
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if !digest.eq_ignore_ascii_case(VGMSTREAM_SHA256) {
        return Err("vgmstream SHA256 검증 실패".into());
    }

    let staging = tool_dir.with_extension(format!("tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|e| format!("vgmstream 임시 경로 생성 실패: {e}"))?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| format!("vgmstream ZIP 열기 실패: {e}"))?;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|e| format!("vgmstream ZIP 항목 실패: {e}"))?;
        let Some(relative) = entry.enclosed_name().map(Path::to_path_buf) else {
            return Err("vgmstream ZIP 경로 검증 실패".into());
        };
        if entry.is_dir() {
            continue;
        }
        let target = staging.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("vgmstream 폴더 생성 실패: {e}"))?;
        }
        let mut out =
            File::create(&target).map_err(|e| format!("vgmstream 파일 생성 실패: {e}"))?;
        std::io::copy(&mut entry, &mut out)
            .map_err(|e| format!("vgmstream 압축 해제 실패: {e}"))?;
    }
    if !staging.join("vgmstream-cli.exe").is_file() {
        return Err("vgmstream-cli.exe 누락".into());
    }
    let _ = fs::remove_dir_all(&tool_dir);
    if let Some(parent) = tool_dir.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("도구 캐시 생성 실패: {e}"))?;
    }
    fs::rename(&staging, &tool_dir).map_err(|e| format!("vgmstream 캐시 확정 실패: {e}"))?;
    Ok(exe)
}

fn extract_ping_audio(
    wad: &Path,
    cache: &Path,
    vgmstream: &Path,
) -> Result<HashMap<String, PathBuf>, String> {
    let banks = wad_read(wad, &[EVENTS_BANK, AUDIO_BANK])?;
    let events = banks
        .get(EVENTS_BANK)
        .ok_or_else(|| "hud_global_events.bnk 누락".to_owned())?;
    let audio = banks
        .get(AUDIO_BANK)
        .ok_or_else(|| "hud_global_audio.bnk 누락".to_owned())?;
    let objects = parse_hirc(events)?;
    let media = parse_media(audio)?;
    let temp = cache.join(format!("tmp-{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp);
    fs::create_dir_all(&temp).map_err(|e| format!("핑 음원 임시 경로 생성 실패: {e}"))?;
    let mut outputs = cached_outputs(cache);
    for (kind, event) in PING_EVENTS {
        if outputs.contains_key(kind) {
            continue;
        }
        let result = (|| -> Result<PathBuf, String> {
            let ids = event_media(&objects, event)?;
            if ids.is_empty() {
                return Err(format!("{event} media 없음"));
            }
            let mut decoded = Vec::new();
            for id in ids {
                let raw = media
                    .get(&id)
                    .ok_or_else(|| format!("Wwise media {id} 누락"))?;
                let wem = temp.join(format!("{kind}-{id}.wem"));
                let wav = temp.join(format!("{kind}-{id}.wav"));
                fs::write(&wem, raw).map_err(|e| format!("WEM 저장 실패: {e}"))?;
                decode_wem(vgmstream, &wem, &wav)?;
                decoded.push(wav);
            }
            let output = cache.join(format!("{kind}.wav"));
            mix_wavs(&decoded, &output)?;
            Ok(output)
        })();
        match result {
            Ok(path) => {
                outputs.insert(kind.to_owned(), path);
            }
            Err(error) => eprintln!("League ping audio extract {kind} failed: {error}"),
        }
    }
    let _ = fs::remove_dir_all(&temp);
    if outputs.is_empty() {
        Err("LoL 핑 음원을 하나도 추출하지 못했습니다.".into())
    } else {
        Ok(outputs)
    }
}

fn decode_wem(vgmstream: &Path, wem: &Path, wav: &Path) -> Result<(), String> {
    #[cfg(windows)]
    use std::os::windows::process::CommandExt;
    let mut command = Command::new(vgmstream);
    command.arg("-o").arg(wav).arg(wem);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let output = command
        .output()
        .map_err(|e| format!("vgmstream 실행 실패: {e}"))?;
    if !output.status.success() || !wav.is_file() {
        return Err(format!(
            "vgmstream decode 실패: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

fn mix_wavs(inputs: &[PathBuf], output: &Path) -> Result<(), String> {
    if inputs.is_empty() {
        return Err("혼합할 WAV가 없습니다.".into());
    }
    if inputs.len() == 1 {
        fs::copy(&inputs[0], output).map_err(|e| format!("WAV 복사 실패: {e}"))?;
        return Ok(());
    }
    let mut tracks = Vec::new();
    let mut spec = None;
    for path in inputs {
        let mut reader = hound::WavReader::open(path).map_err(|e| format!("WAV 열기 실패: {e}"))?;
        let current = reader.spec();
        if current.sample_format != hound::SampleFormat::Int || current.bits_per_sample != 16 {
            return Err("vgmstream WAV가 PCM16이 아닙니다.".into());
        }
        if let Some(expected) = spec {
            if expected != current {
                return Err("Wwise layer WAV 형식 불일치".into());
            }
        } else {
            spec = Some(current);
        }
        tracks.push(
            reader
                .samples::<i16>()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("WAV 샘플 읽기 실패: {e}"))?,
        );
    }
    let spec = spec.ok_or_else(|| "WAV spec 누락".to_owned())?;
    let len = tracks.iter().map(Vec::len).max().unwrap_or(0);
    let mut mixed = vec![0_i32; len];
    for track in tracks {
        for (i, sample) in track.into_iter().enumerate() {
            mixed[i] += i32::from(sample);
        }
    }
    let peak = mixed.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0) as f64;
    let target = f64::from(i16::MAX) * 0.89;
    let gain = if peak > target && peak > 0.0 {
        target / peak
    } else {
        1.0
    };
    let mut writer =
        hound::WavWriter::create(output, spec).map_err(|e| format!("WAV 출력 생성 실패: {e}"))?;
    for sample in mixed {
        let value = (f64::from(sample) * gain)
            .round()
            .clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
        writer
            .write_sample(value)
            .map_err(|e| format!("WAV 출력 실패: {e}"))?;
    }
    writer
        .finalize()
        .map_err(|e| format!("WAV 마무리 실패: {e}"))?;
    Ok(())
}

fn wad_read(wad: &Path, wanted: &[&str]) -> Result<HashMap<String, Vec<u8>>, String> {
    let wanted_by_hash: HashMap<u64, &str> = wanted
        .iter()
        .map(|path| (xxh64(path.to_ascii_lowercase().as_bytes(), 0), *path))
        .collect();
    let mut file = File::open(wad).map_err(|e| format!("WAD 열기 실패: {e}"))?;
    let mut header = [0_u8; 4];
    file.read_exact(&mut header)
        .map_err(|e| format!("WAD header 읽기 실패: {e}"))?;
    if &header[..2] != b"RW" || header[2] < 3 {
        return Err(format!("지원하지 않는 WAD 버전: {}", header[2]));
    }
    file.seek(SeekFrom::Current(264))
        .map_err(|e| format!("WAD header seek 실패: {e}"))?;
    let count = read_u32_from(&mut file)?;
    let mut entries = Vec::new();
    for _ in 0..count {
        let path_hash = read_u64_from(&mut file)?;
        let offset = read_u32_from(&mut file)?;
        let compressed_size = read_u32_from(&mut file)?;
        let _size = read_u32_from(&mut file)?;
        let mut small = [0_u8; 4];
        file.read_exact(&mut small)
            .map_err(|e| format!("WAD entry 읽기 실패: {e}"))?;
        let kind = small[0] & 0x0f;
        let _checksum = read_u64_from(&mut file)?;
        if wanted_by_hash.contains_key(&path_hash) {
            entries.push((path_hash, u64::from(offset), compressed_size as usize, kind));
        }
    }
    let mut out = HashMap::new();
    for (path_hash, offset, compressed_size, kind) in entries {
        let path = wanted_by_hash[&path_hash];
        file.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("WAD data seek 실패: {e}"))?;
        let mut raw = vec![0_u8; compressed_size];
        file.read_exact(&mut raw)
            .map_err(|e| format!("WAD data 읽기 실패: {e}"))?;
        let data = match kind {
            0 => raw,
            3 | 4 => decompress_wad_entry(&raw)?,
            other => return Err(format!("{path}: 지원하지 않는 WAD entry type {other}")),
        };
        out.insert(path.to_owned(), data);
    }
    let missing: Vec<&str> = wanted
        .iter()
        .copied()
        .filter(|path| !out.contains_key(*path))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "WAD에서 bank를 찾지 못했습니다: {}",
            missing.join(", ")
        ));
    }
    Ok(out)
}

fn decompress_wad_entry(raw: &[u8]) -> Result<Vec<u8>, String> {
    const ZSTD_MAGIC: &[u8; 4] = b"\x28\xb5\x2f\xfd";
    let start = raw
        .windows(4)
        .position(|window| window == ZSTD_MAGIC)
        .ok_or_else(|| "WAD zstd frame 누락".to_owned())?;
    let mut decoder = StreamingDecoder::new(Cursor::new(&raw[start..]))
        .map_err(|e| format!("WAD zstd decoder 생성 실패: {e}"))?;
    let mut decoded = Vec::new();
    decoder
        .read_to_end(&mut decoded)
        .map_err(|e| format!("WAD zstd 해제 실패: {e}"))?;
    if start == 0 {
        Ok(decoded)
    } else {
        let mut joined = raw[..start].to_vec();
        joined.extend_from_slice(&decoded);
        Ok(joined)
    }
}

fn parse_hirc(data: &[u8]) -> Result<HashMap<u32, (u8, Vec<u8>)>, String> {
    let mut objects = HashMap::new();
    for (tag, body) in bnk_sections(data)? {
        if tag != b"HIRC" {
            continue;
        }
        if body.len() < 4 {
            return Err("HIRC header가 짧습니다.".into());
        }
        let count = le_u32(&body[0..4])? as usize;
        let mut offset = 4;
        for _ in 0..count {
            if offset + 9 > body.len() {
                return Err("HIRC object header가 잘렸습니다.".into());
            }
            let object_type = body[offset];
            let size = le_u32(&body[offset + 1..offset + 5])? as usize;
            let object_id = le_u32(&body[offset + 5..offset + 9])?;
            if size < 4 || offset + 5 + size > body.len() {
                return Err("HIRC object size 오류".into());
            }
            objects.insert(
                object_id,
                (object_type, body[offset + 9..offset + 5 + size].to_vec()),
            );
            offset += 5 + size;
        }
    }
    Ok(objects)
}

fn parse_media(data: &[u8]) -> Result<HashMap<u32, Vec<u8>>, String> {
    let mut index = Vec::new();
    let mut blob = &[][..];
    for (tag, body) in bnk_sections(data)? {
        if tag == b"DIDX" {
            if body.len() % 12 != 0 {
                return Err("Wwise DIDX 크기 오류".into());
            }
            for row in body.chunks_exact(12) {
                index.push((
                    le_u32(&row[0..4])?,
                    le_u32(&row[4..8])? as usize,
                    le_u32(&row[8..12])? as usize,
                ));
            }
        } else if tag == b"DATA" {
            blob = body;
        }
    }
    let mut media = HashMap::new();
    for (id, offset, size) in index {
        let end = offset
            .checked_add(size)
            .ok_or_else(|| "Wwise media overflow".to_owned())?;
        if end > blob.len() {
            return Err("Wwise media 범위 오류".into());
        }
        media.insert(id, blob[offset..end].to_vec());
    }
    Ok(media)
}

fn event_media(
    objects: &HashMap<u32, (u8, Vec<u8>)>,
    event_name: &str,
) -> Result<Vec<u32>, String> {
    const SOUND: u8 = 2;
    const ACTION: u8 = 3;
    const EVENT: u8 = 4;
    const RANSEQ: u8 = 5;
    const SWITCH: u8 = 6;
    const ACTOR_MIXER: u8 = 7;
    const LAYER: u8 = 9;
    const AUDIO_DEVICE: u8 = 14;
    fn walk(id: u32, objects: &HashMap<u32, (u8, Vec<u8>)>, seen: &mut HashSet<u32>) -> Vec<u32> {
        if !seen.insert(id) {
            return Vec::new();
        }
        let Some((kind, body)) = objects.get(&id) else {
            return Vec::new();
        };
        match *kind {
            SOUND if body.len() >= 9 => le_u32(&body[5..9]).ok().into_iter().collect(),
            ACTION if body.len() >= 6 => le_u32(&body[2..6])
                .ok()
                .map(|target| walk(target, objects, seen))
                .unwrap_or_default(),
            EVENT | RANSEQ | SWITCH | LAYER => {
                let mut refs = BTreeSet::new();
                for bytes in body.windows(4) {
                    if let Ok(reference) = le_u32(bytes) {
                        if let Some((ref_kind, _)) = objects.get(&reference) {
                            if *ref_kind != ACTOR_MIXER && *ref_kind != AUDIO_DEVICE {
                                refs.insert(reference);
                            }
                        }
                    }
                }
                refs.into_iter()
                    .flat_map(|reference| walk(reference, objects, seen))
                    .collect()
            }
            _ => Vec::new(),
        }
    }
    let event_id = fnv1_32(event_name);
    if !objects.contains_key(&event_id) {
        return Err(format!("Wwise event 없음: {event_name}"));
    }
    let mut unique = Vec::new();
    for id in walk(event_id, objects, &mut HashSet::new()) {
        if !unique.contains(&id) {
            unique.push(id);
        }
    }
    Ok(unique)
}

fn fnv1_32(value: &str) -> u32 {
    value
        .to_ascii_lowercase()
        .bytes()
        .fold(2_166_136_261_u32, |hash, byte| {
            hash.wrapping_mul(16_777_619) ^ u32::from(byte)
        })
}

fn bnk_sections(data: &[u8]) -> Result<Vec<(&[u8; 4], &[u8])>, String> {
    let mut result = Vec::new();
    let mut offset = 0;
    while offset + 8 <= data.len() {
        let tag: &[u8; 4] = data[offset..offset + 4]
            .try_into()
            .map_err(|_| "BNK tag 오류".to_owned())?;
        let size = le_u32(&data[offset + 4..offset + 8])? as usize;
        let start = offset + 8;
        let end = start
            .checked_add(size)
            .ok_or_else(|| "BNK section overflow".to_owned())?;
        if end > data.len() {
            return Err("BNK section 범위 오류".into());
        }
        result.push((tag, &data[start..end]));
        offset = end;
    }
    Ok(result)
}

fn le_u32(bytes: &[u8]) -> Result<u32, String> {
    let bytes: [u8; 4] = bytes
        .get(..4)
        .ok_or_else(|| "u32 범위 오류".to_owned())?
        .try_into()
        .map_err(|_| "u32 변환 오류".to_owned())?;
    Ok(u32::from_le_bytes(bytes))
}
fn read_u32_from<R: Read>(reader: &mut R) -> Result<u32, String> {
    let mut bytes = [0_u8; 4];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| format!("u32 읽기 실패: {e}"))?;
    Ok(u32::from_le_bytes(bytes))
}
fn read_u64_from<R: Read>(reader: &mut R) -> Result<u64, String> {
    let mut bytes = [0_u8; 8];
    reader
        .read_exact(&mut bytes)
        .map_err(|e| format!("u64 읽기 실패: {e}"))?;
    Ok(u64::from_le_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ping_event_contract_matches_league_wwise_names() {
        assert_eq!(
            PING_EVENTS[0],
            ("retreat", "Play_sfx_hud_base_Pings_SRP_12")
        );
        assert_eq!(
            PING_EVENTS[7],
            ("enemy_vision", "Play_sfx_hud_base_Pings_AreaIsWarded")
        );
        assert_eq!(
            fnv1_32("Play_sfx_hud_base_Pings_MIA"),
            fnv1_32("play_sfx_hud_base_pings_mia")
        );
    }
}
