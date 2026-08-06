use std::ffi::{CStr, CString, c_char, c_float, c_int, c_uint, c_ulonglong, c_void};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::ptr;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::UNIX_EPOCH;

use color_eyre::eyre::{Result, WrapErr, eyre};
use fs2::FileExt;
use libloading::Library;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::events::TranscriptPhase;

const HEADER_VERSION: c_int = 20_000;
const SMALL_STREAMING: c_uint = 4;
const FORCE_UPDATE: c_uint = 1;
const MODEL_BASE_URL: &str = "https://download.moonshine.ai/model/small-streaming-en/quantized";
const MODEL_COMPONENTS: &[ModelComponent] = &[
    ModelComponent::new(
        "adapter.ort",
        2_867_424,
        "d8493e0ac76a198b309a8be6f74b3101e235f773ffe5d6b378278cd7e4177992",
    ),
    ModelComponent::new(
        "cross_kv.ort",
        5_298_736,
        "6e57d1361717e00d73336a0c3beafedae784b1e537905ad253dee33db4007466",
    ),
    ModelComponent::new(
        "decoder_kv.ort",
        81_435_904,
        "d5adfcfaa6e582144791f1568bd0f683852c7bfbb8c79acad97499da05e4ffcf",
    ),
    ModelComponent::new(
        "decoder_kv_with_attention.ort",
        81_380_336,
        "2ac12d0b1ab1459ae2572b0d8f0a359a79ac83ad0a5de0b40bdb33c9357048ee",
    ),
    ModelComponent::new(
        "encoder.ort",
        43_853_224,
        "3b21d02eff6aa5651524ada4271d37c1d7bba4eb3d256415074f2cfdbaeb526a",
    ),
    ModelComponent::new(
        "frontend.ort",
        30_984_200,
        "e086451043c1c8652a9614e4a4a81d5807221b611584a3cf31f73779d5900003",
    ),
    ModelComponent::new(
        "streaming_config.json",
        512,
        "26f02b6afb22d60871a5efd85c3d38e569cc0ddb6c5eb6e93d3260152ae8a47a",
    ),
    ModelComponent::new(
        "tokenizer.bin",
        249_974,
        "6884b35fd6377d4c4d32336a0bc152f36b64d1e45b6503683cdc238250a8472d",
    ),
];
// Moonshine's native handle registry does not synchronize lookups against
// transcriber insertion/removal. Lifecycle calls are exclusive; inference on
// already-created transcribers may remain concurrent.
static NATIVE_LOCK: RwLock<()> = RwLock::new(());

struct ModelComponent {
    filename: &'static str,
    bytes: u64,
    sha256: &'static str,
}

#[derive(Deserialize, Serialize)]
struct VerificationReceipt {
    files: Vec<VerifiedFile>,
}

#[derive(Deserialize, Serialize)]
struct VerifiedFile {
    filename: String,
    bytes: u64,
    sha256: String,
    modified_ns: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct MoonshineConfig {
    pub transcription_interval_ms: u32,
    pub word_timestamps: bool,
}

impl Default for MoonshineConfig {
    fn default() -> Self {
        Self {
            transcription_interval_ms: 200,
            word_timestamps: false,
        }
    }
}

impl ModelComponent {
    const fn new(filename: &'static str, bytes: u64, sha256: &'static str) -> Self {
        Self {
            filename,
            bytes,
            sha256,
        }
    }
}

pub fn model_installed() -> bool {
    model_path().is_ok_and(|path| verification_receipt_matches(&path))
}

pub fn install_model() -> Result<PathBuf> {
    let destination = model_path()?;
    let parent = destination
        .parent()
        .ok_or_else(|| eyre!("Moonshine model path has no parent"))?;
    fs::create_dir_all(parent)?;
    let lock = File::create(parent.join(".download.lock"))?;
    lock.lock_exclusive()?;
    if model_installed() {
        return Ok(destination);
    }
    if destination.exists()
        && MODEL_COMPONENTS.iter().all(|component| {
            verify_component(&destination.join(component.filename), component).is_ok()
        })
    {
        write_verification_receipt(&destination)?;
        return Ok(destination);
    }

    let staging = parent.join("quantized.partial");
    fs::create_dir_all(&staging)?;
    for component in MODEL_COMPONENTS {
        let completed = staging.join(component.filename);
        if verify_component(&completed, component).is_ok() {
            continue;
        }
        let partial = staging.join(format!("{}.download", component.filename));
        if fs::metadata(&partial).is_ok_and(|metadata| metadata.len() >= component.bytes) {
            fs::remove_file(&partial)?;
        }
        let output = Command::new("/usr/bin/curl")
            .args([
                "--fail",
                "--location",
                "--retry",
                "3",
                "--continue-at",
                "-",
                "--silent",
                "--show-error",
                "--output",
            ])
            .arg(&partial)
            .arg(format!("{MODEL_BASE_URL}/{}", component.filename))
            .output()
            .wrap_err("could not start Moonshine model download")?;
        if !output.status.success() {
            return Err(eyre!(
                "Moonshine model download failed with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        verify_component(&partial, component)?;
        File::open(&partial)?.sync_all()?;
        fs::rename(partial, completed)?;
    }
    File::open(&staging)?.sync_all()?;

    let previous = parent.join("quantized.replaced");
    if previous.exists() {
        fs::remove_dir_all(&previous)?;
    }
    if destination.exists() {
        fs::rename(&destination, &previous)?;
    }
    fs::rename(&staging, &destination)?;
    File::open(parent)?.sync_all()?;
    write_verification_receipt(&destination)?;
    if previous.exists() {
        fs::remove_dir_all(previous)?;
    }
    Ok(destination)
}

fn verification_receipt_path(model: &Path) -> PathBuf {
    model.join(".verified.json")
}

fn modified_ns(metadata: &fs::Metadata) -> Option<u64> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .try_into()
        .ok()
}

fn verification_receipt_matches(model: &Path) -> bool {
    let Ok(bytes) = fs::read(verification_receipt_path(model)) else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<VerificationReceipt>(&bytes) else {
        return false;
    };
    receipt.files.len() == MODEL_COMPONENTS.len()
        && MODEL_COMPONENTS.iter().all(|component| {
            let Some(file) = receipt
                .files
                .iter()
                .find(|file| file.filename == component.filename)
            else {
                return false;
            };
            let Ok(metadata) = fs::metadata(model.join(component.filename)) else {
                return false;
            };
            file.bytes == component.bytes
                && file.sha256 == component.sha256
                && metadata.len() == component.bytes
                && modified_ns(&metadata) == Some(file.modified_ns)
        })
}

fn write_verification_receipt(model: &Path) -> Result<()> {
    let files = MODEL_COMPONENTS
        .iter()
        .map(|component| {
            let metadata = fs::metadata(model.join(component.filename))?;
            Ok(VerifiedFile {
                filename: component.filename.into(),
                bytes: component.bytes,
                sha256: component.sha256.into(),
                modified_ns: modified_ns(&metadata)
                    .ok_or_else(|| eyre!("Moonshine model modification time is unavailable"))?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let destination = verification_receipt_path(model);
    let temporary = destination.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec(&VerificationReceipt { files })?,
    )?;
    File::open(&temporary)?.sync_all()?;
    fs::rename(temporary, destination)?;
    File::open(model)?.sync_all()?;
    Ok(())
}

fn model_path() -> Result<PathBuf> {
    Ok(dirs::cache_dir()
        .ok_or_else(|| eyre!("cache directory is unavailable"))?
        .join("moonshine_voice/download.moonshine.ai/model/small-streaming-en/quantized"))
}

fn verify_component(path: &Path, component: &ModelComponent) -> Result<()> {
    if fs::metadata(path)?.len() != component.bytes {
        return Err(eyre!("invalid byte length for {}", component.filename));
    }
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 1024 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let actual = digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if actual != component.sha256 {
        return Err(eyre!("checksum mismatch for {}", component.filename));
    }
    Ok(())
}

#[repr(C)]
struct MoonshineOption {
    name: *const c_char,
    value: *const c_char,
}

#[repr(C)]
struct TranscriptWord {
    text: *const c_char,
    start: c_float,
    end: c_float,
    confidence: c_float,
}

#[repr(C)]
struct TranscriptLine {
    text: *const c_char,
    audio_data: *const c_float,
    audio_data_count: c_ulonglong,
    start_time: c_float,
    duration: c_float,
    id: c_ulonglong,
    is_complete: i8,
    is_updated: i8,
    is_new: i8,
    has_text_changed: i8,
    have_speakers_changed: i8,
    speaker_spans: *const c_void,
    speaker_span_count: c_ulonglong,
    last_transcription_latency_ms: c_uint,
    words: *const TranscriptWord,
    word_count: c_ulonglong,
}

#[repr(C)]
struct Transcript {
    lines: *mut TranscriptLine,
    line_count: c_ulonglong,
}

type LoadTranscriber = unsafe extern "C" fn(
    *const c_char,
    c_uint,
    *const MoonshineOption,
    c_ulonglong,
    c_int,
) -> c_int;
type GetVersion = unsafe extern "C" fn() -> c_int;
type FreeTranscriber = unsafe extern "C" fn(c_int);
type CreateStream = unsafe extern "C" fn(c_int, c_uint) -> c_int;
type FreeStream = unsafe extern "C" fn(c_int, c_int) -> c_int;
type StartStream = unsafe extern "C" fn(c_int, c_int) -> c_int;
type StopStream = unsafe extern "C" fn(c_int, c_int) -> c_int;
type AddAudio =
    unsafe extern "C" fn(c_int, c_int, *const c_float, c_ulonglong, c_int, c_uint) -> c_int;
type TranscribeStream = unsafe extern "C" fn(c_int, c_int, c_uint, *mut *mut Transcript) -> c_int;
type ErrorToString = unsafe extern "C" fn(c_int) -> *const c_char;

struct Functions {
    get_version: GetVersion,
    free_transcriber: FreeTranscriber,
    create_stream: CreateStream,
    free_stream: FreeStream,
    start_stream: StartStream,
    stop_stream: StopStream,
    add_audio: AddAudio,
    transcribe_stream: TranscribeStream,
    error_to_string: ErrorToString,
}

pub struct RecognitionUpdate {
    pub phase: TranscriptPhase,
    pub latency_ms: u32,
    pub line_id: u64,
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
    #[cfg_attr(not(debug_assertions), allow(dead_code))]
    pub words: Vec<RecognitionWord>,
}

#[derive(Clone, Debug)]
#[cfg_attr(not(debug_assertions), allow(dead_code))]
pub struct RecognitionWord {
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub confidence: f32,
}

pub struct Moonshine {
    functions: Functions,
    transcriber: c_int,
    streams: Vec<c_int>,
    stream_active: Vec<bool>,
    // Function pointers above remain valid only while this library is loaded.
    _library: Library,
}

impl Moonshine {
    pub fn load(project_root: &Path) -> Result<Self> {
        let config = MoonshineConfig::default();
        let moonshine = Self::load_with_config(project_root, 1, config)?;
        tracing::info!(
            moonshine_api_version = HEADER_VERSION,
            model = "small-streaming-en-quantized",
            transcription_interval_ms = config.transcription_interval_ms,
            word_timestamps = config.word_timestamps,
            provider = "cpu",
            "Moonshine command recognizer loaded"
        );
        Ok(moonshine)
    }

    pub fn load_with_streams(project_root: &Path, stream_count: usize) -> Result<Self> {
        Self::load_with_config(project_root, stream_count, MoonshineConfig::default())
    }

    pub fn load_with_config(
        project_root: &Path,
        stream_count: usize,
        config: MoonshineConfig,
    ) -> Result<Self> {
        if stream_count == 0 {
            return Err(eyre!("Moonshine requires at least one stream"));
        }
        let library_path = find_library(project_root)?;
        let model_path = model_path()?;
        if !model_path.exists() {
            return Err(eyre!(
                "Moonshine command model is missing at {}. Finish setup in HEX.",
                model_path.display()
            ));
        }

        // SAFETY: The library path comes from the pinned Moonshine wheel. Every
        // symbol is copied into a function pointer while the Library remains
        // owned by this value, so none can outlive the dynamic library.
        unsafe {
            let library = Library::new(&library_path)
                .wrap_err_with(|| format!("could not load {}", library_path.display()))?;
            let load: LoadTranscriber = *library.get(b"moonshine_load_transcriber_from_files\0")?;
            let functions = Functions {
                get_version: *library.get(b"moonshine_get_version\0")?,
                free_transcriber: *library.get(b"moonshine_free_transcriber\0")?,
                create_stream: *library.get(b"moonshine_create_stream\0")?,
                free_stream: *library.get(b"moonshine_free_stream\0")?,
                start_stream: *library.get(b"moonshine_start_stream\0")?,
                stop_stream: *library.get(b"moonshine_stop_stream\0")?,
                add_audio: *library.get(b"moonshine_transcribe_add_audio_to_stream\0")?,
                transcribe_stream: *library.get(b"moonshine_transcribe_stream\0")?,
                error_to_string: *library.get(b"moonshine_error_to_string\0")?,
            };
            let version = (functions.get_version)();
            if version != HEADER_VERSION {
                return Err(eyre!(
                    "incompatible Moonshine API version: expected {HEADER_VERSION}, loaded {version}"
                ));
            }
            let _native = write_native();
            let path = CString::new(model_path.to_string_lossy().as_bytes())?;
            let option_values = vec![
                (
                    "transcription_interval",
                    format!(
                        "{:.3}",
                        f64::from(config.transcription_interval_ms) / 1_000.0
                    ),
                ),
                ("vad_threshold", "0.5".into()),
                ("vad_window_duration", "0.5".into()),
                ("vad_look_behind_sample_count", "8192".into()),
                ("vad_max_segment_duration", "15".into()),
                ("return_audio_data", "false".into()),
                ("word_timestamps", config.word_timestamps.to_string()),
                ("identify_speakers", "false".into()),
                ("ort_provider", "cpu".into()),
            ];
            let option_strings = option_values
                .into_iter()
                .map(|(name, value)| Ok((CString::new(name)?, CString::new(value)?)))
                .collect::<Result<Vec<_>>>()?;
            let options = option_strings
                .iter()
                .map(|(name, value)| MoonshineOption {
                    name: name.as_ptr(),
                    value: value.as_ptr(),
                })
                .collect::<Vec<_>>();
            let transcriber = load(
                path.as_ptr(),
                SMALL_STREAMING,
                options.as_ptr(),
                options.len() as u64,
                HEADER_VERSION,
            );
            check_handle(&functions, transcriber, "load transcriber")?;
            let mut streams = Vec::with_capacity(stream_count);
            for _ in 0..stream_count {
                let stream = (functions.create_stream)(transcriber, 0);
                if let Err(error) = check_handle(&functions, stream, "create stream") {
                    free_streams(
                        &functions,
                        transcriber,
                        &streams,
                        &vec![true; streams.len()],
                    );
                    (functions.free_transcriber)(transcriber);
                    return Err(error);
                }
                if let Err(error) = check(
                    &functions,
                    (functions.start_stream)(transcriber, stream),
                    "start stream",
                ) {
                    (functions.free_stream)(transcriber, stream);
                    free_streams(
                        &functions,
                        transcriber,
                        &streams,
                        &vec![true; streams.len()],
                    );
                    (functions.free_transcriber)(transcriber);
                    return Err(error);
                }
                streams.push(stream);
            }

            Ok(Self {
                functions,
                transcriber,
                stream_active: vec![true; streams.len()],
                streams,
                _library: library,
            })
        }
    }

    pub fn add_audio(&mut self, samples: &[f32], sample_rate: u32) -> Result<()> {
        self.add_audio_to(0, samples, sample_rate)
    }

    pub fn add_audio_to(
        &mut self,
        stream_index: usize,
        samples: &[f32],
        sample_rate: u32,
    ) -> Result<()> {
        let stream = self.stream(stream_index)?;
        let _native = read_native();
        // SAFETY: Moonshine consumes the slice during this call and does not
        // retain it. Both handles are owned and live.
        let code = unsafe {
            (self.functions.add_audio)(
                self.transcriber,
                stream,
                samples.as_ptr(),
                samples.len() as u64,
                sample_rate as c_int,
                0,
            )
        };
        check(&self.functions, code, "add audio")
    }

    pub fn update(&mut self) -> Result<Vec<RecognitionUpdate>> {
        self.update_stream(0)
    }

    pub fn update_stream(&mut self, stream_index: usize) -> Result<Vec<RecognitionUpdate>> {
        let stream = self.stream(stream_index)?;
        let _native = read_native();
        self.read_transcript(stream, 0)
    }

    pub fn finish_stream(&mut self, stream_index: usize) -> Result<Vec<RecognitionUpdate>> {
        let stream = self.stream(stream_index)?;
        let _native = read_native();
        if self.stream_active[stream_index] {
            let stop = unsafe { (self.functions.stop_stream)(self.transcriber, stream) };
            check(&self.functions, stop, "stop stream")?;
            self.stream_active[stream_index] = false;
        }
        let mut updates = self.read_transcript(stream, FORCE_UPDATE)?;
        updates.extend(self.read_transcript(stream, 0)?);
        Ok(updates)
    }

    fn read_transcript(&self, stream: c_int, flags: c_uint) -> Result<Vec<RecognitionUpdate>> {
        let mut transcript = ptr::null_mut();
        // SAFETY: Moonshine initializes transcript to memory owned by the live
        // transcriber and guarantees validity until the next transcriber call.
        let code = unsafe {
            (self.functions.transcribe_stream)(self.transcriber, stream, flags, &mut transcript)
        };
        check(&self.functions, code, "transcribe stream")?;
        if transcript.is_null() {
            return Ok(Vec::new());
        }

        // SAFETY: The C API supplies line_count contiguous TranscriptLine
        // values and NUL-terminated UTF-8 text for every line.
        let transcript = unsafe { &*transcript };
        if transcript.line_count == 0 {
            return Ok(Vec::new());
        }
        if transcript.lines.is_null() {
            return Err(eyre!("Moonshine returned lines without storage"));
        }
        let lines =
            unsafe { std::slice::from_raw_parts(transcript.lines, transcript.line_count as usize) };
        Ok(lines
            .iter()
            .filter(|line| line.is_new != 0 || line.is_updated != 0)
            .map(|line| RecognitionUpdate {
                phase: if line.is_complete != 0 {
                    TranscriptPhase::Completed
                } else if line.is_new != 0 {
                    TranscriptPhase::Started
                } else {
                    TranscriptPhase::Updated
                },
                latency_ms: line.last_transcription_latency_ms,
                line_id: line.id,
                start_ms: (line.start_time.max(0.0) * 1_000.0) as u64,
                end_ms: ((line.start_time + line.duration).max(0.0) * 1_000.0) as u64,
                text: if line.text.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(line.text) }
                        .to_string_lossy()
                        .into_owned()
                },
                words: if line.words.is_null() || line.word_count == 0 {
                    Vec::new()
                } else {
                    unsafe { std::slice::from_raw_parts(line.words, line.word_count as usize) }
                        .iter()
                        .map(|word| RecognitionWord {
                            text: if word.text.is_null() {
                                String::new()
                            } else {
                                unsafe { CStr::from_ptr(word.text) }
                                    .to_string_lossy()
                                    .into_owned()
                            },
                            start_ms: (word.start.max(0.0) * 1_000.0) as u64,
                            end_ms: (word.end.max(0.0) * 1_000.0) as u64,
                            confidence: word.confidence,
                        })
                        .collect()
                },
            })
            .collect())
    }

    pub fn reset_stream(&mut self) -> Result<()> {
        self.reset_stream_at(0)
    }

    fn reset_stream_at(&mut self, stream_index: usize) -> Result<()> {
        let current = self.stream(stream_index)?;
        let _native = read_native();
        // SAFETY: A replacement is fully started before the current stream is
        // discarded, so every error leaves this value with a live stream.
        unsafe {
            let replacement = (self.functions.create_stream)(self.transcriber, 0);
            check_handle(&self.functions, replacement, "create stream")?;
            if let Err(error) = check(
                &self.functions,
                (self.functions.start_stream)(self.transcriber, replacement),
                "start stream",
            ) {
                (self.functions.free_stream)(self.transcriber, replacement);
                return Err(error);
            }
            if let Err(error) = check(
                &self.functions,
                (self.functions.stop_stream)(self.transcriber, current),
                "stop stream",
            ) {
                (self.functions.stop_stream)(self.transcriber, replacement);
                (self.functions.free_stream)(self.transcriber, replacement);
                return Err(error);
            }
            let free_result = check(
                &self.functions,
                (self.functions.free_stream)(self.transcriber, current),
                "free stream",
            );
            self.streams[stream_index] = replacement;
            self.stream_active[stream_index] = true;
            free_result
        }
    }

    fn stream(&self, index: usize) -> Result<c_int> {
        self.streams
            .get(index)
            .copied()
            .ok_or_else(|| eyre!("Moonshine stream {index} does not exist"))
    }
}

impl Drop for Moonshine {
    fn drop(&mut self) {
        // SAFETY: Drop is the final owner of these handles and ignores cleanup
        // errors because there is no recovery path during destruction.
        let _native = write_native();
        unsafe {
            free_streams(
                &self.functions,
                self.transcriber,
                &self.streams,
                &self.stream_active,
            );
            (self.functions.free_transcriber)(self.transcriber);
        }
    }
}

fn read_native() -> RwLockReadGuard<'static, ()> {
    NATIVE_LOCK
        .read()
        .unwrap_or_else(|error| error.into_inner())
}

fn write_native() -> RwLockWriteGuard<'static, ()> {
    NATIVE_LOCK
        .write()
        .unwrap_or_else(|error| error.into_inner())
}

unsafe fn free_streams(
    functions: &Functions,
    transcriber: c_int,
    streams: &[c_int],
    active: &[bool],
) {
    for (&stream, &active) in streams.iter().zip(active) {
        unsafe {
            if active {
                (functions.stop_stream)(transcriber, stream);
            }
            (functions.free_stream)(transcriber, stream);
        }
    }
}

fn find_library(project_root: &Path) -> Result<PathBuf> {
    #[cfg(target_os = "macos")]
    const LIBRARY_NAME: &str = "libmoonshine.dylib";
    #[cfg(target_os = "linux")]
    const LIBRARY_NAME: &str = "libmoonshine.so";

    #[cfg(target_os = "macos")]
    if let Ok(executable) = std::env::current_exe()
        && let Some(contents) = executable.parent().and_then(Path::parent)
    {
        let bundled = contents.join("Frameworks").join(LIBRARY_NAME);
        if bundled.exists() {
            return Ok(bundled);
        }
    }
    let lib = project_root.join(".venv/lib");
    for python in
        fs::read_dir(&lib).wrap_err_with(|| format!("could not read {}", lib.display()))?
    {
        let candidate = python?
            .path()
            .join("site-packages/moonshine_voice")
            .join(LIBRARY_NAME);
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(eyre!(
        "Moonshine native library not found. Run ./scripts/setup.sh"
    ))
}

fn check(functions: &Functions, code: c_int, operation: &str) -> Result<()> {
    if code == 0 {
        Ok(())
    } else {
        Err(eyre!("{operation}: {}", error_message(functions, code)))
    }
}

fn check_handle(functions: &Functions, handle: c_int, operation: &str) -> Result<()> {
    if handle >= 0 {
        Ok(())
    } else {
        Err(eyre!("{operation}: {}", error_message(functions, handle)))
    }
}

fn error_message(functions: &Functions, code: c_int) -> String {
    // SAFETY: Moonshine returns a static NUL-terminated error string.
    let message = unsafe { (functions.error_to_string)(code) };
    if message.is_null() {
        format!("unknown error {code}")
    } else {
        unsafe { CStr::from_ptr(message) }
            .to_string_lossy()
            .into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::sync::{Arc, Barrier};
    use std::time::SystemTime;

    #[test]
    fn native_structs_match_the_moonshine_lp64_abi() {
        assert_eq!(std::mem::size_of::<MoonshineOption>(), 16);
        assert_eq!(std::mem::size_of::<TranscriptWord>(), 24);
        assert_eq!(std::mem::size_of::<TranscriptLine>(), 88);
        assert_eq!(std::mem::size_of::<Transcript>(), 16);
    }

    #[test]
    fn verification_receipt_is_invalidated_when_a_component_changes() {
        let directory = std::env::temp_dir().join(format!(
            "hex-moonshine-receipt-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&directory).unwrap();
        for component in MODEL_COMPONENTS {
            OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(directory.join(component.filename))
                .unwrap()
                .set_len(component.bytes)
                .unwrap();
        }
        write_verification_receipt(&directory).unwrap();
        assert!(verification_receipt_matches(&directory));

        OpenOptions::new()
            .write(true)
            .open(directory.join(MODEL_COMPONENTS[0].filename))
            .unwrap()
            .set_len(MODEL_COMPONENTS[0].bytes - 1)
            .unwrap();
        assert!(!verification_receipt_matches(&directory));
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    #[ignore = "requires the installed Moonshine model and native library"]
    fn installed_model_has_a_valid_verification_receipt() {
        install_model().unwrap();
        assert!(model_installed());
    }

    #[test]
    #[ignore = "requires the installed Moonshine model and native library"]
    fn one_model_can_drive_two_independent_streams() {
        let mut moonshine =
            Moonshine::load_with_streams(Path::new(env!("CARGO_MANIFEST_DIR")), 2).unwrap();
        let silence = vec![0.0; 16_000];
        moonshine.add_audio_to(0, &silence, 16_000).unwrap();
        moonshine.add_audio_to(1, &silence, 16_000).unwrap();
        moonshine.update_stream(0).unwrap();
        moonshine.update_stream(1).unwrap();
        moonshine.finish_stream(0).unwrap();
        moonshine.finish_stream(1).unwrap();
        assert!(moonshine.add_audio_to(2, &silence, 16_000).is_err());
    }

    #[test]
    #[ignore = "requires the installed Moonshine model and native library"]
    fn transcriber_lifecycle_is_serialized_against_native_inference() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut recognition = Moonshine::load(root).unwrap();
        let barrier = Arc::new(Barrier::new(2));
        std::thread::scope(|scope| {
            let inference_barrier = barrier.clone();
            scope.spawn(move || {
                inference_barrier.wait();
                let silence = vec![0.0; 3_200];
                for _ in 0..10 {
                    recognition.add_audio(&silence, 16_000).unwrap();
                    recognition.update().unwrap();
                }
            });
            scope.spawn(move || {
                barrier.wait();
                drop(Moonshine::load(root).unwrap());
            });
        });
    }
}
