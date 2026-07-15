use std::ffi::{CStr, CString, c_char, c_float, c_int, c_uint, c_ulonglong, c_void};
use std::fs;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

use color_eyre::eyre::{Result, WrapErr, eyre};
use libloading::Library;

use crate::events::TranscriptPhase;

const HEADER_VERSION: c_int = 20_000;
const MEDIUM_STREAMING: c_uint = 5;
const FORCE_UPDATE: c_uint = 1;
// Moonshine's native handle registry does not synchronize lookups against
// transcriber insertion/removal. Lifecycle calls are exclusive; inference on
// already-created transcribers may remain concurrent.
static NATIVE_LOCK: RwLock<()> = RwLock::new(());

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
    words: *const c_void,
    word_count: c_ulonglong,
}

#[repr(C)]
struct Transcript {
    lines: *mut TranscriptLine,
    line_count: c_ulonglong,
}

type LoadTranscriber =
    unsafe extern "C" fn(*const c_char, c_uint, *const c_void, c_ulonglong, c_int) -> c_int;
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
        Self::load_with_streams(project_root, 1)
    }

    pub fn load_with_streams(project_root: &Path, stream_count: usize) -> Result<Self> {
        if stream_count == 0 {
            return Err(eyre!("Moonshine requires at least one stream"));
        }
        let library_path = find_library(project_root)?;
        let model_path = dirs::cache_dir()
            .ok_or_else(|| eyre!("macOS cache directory is unavailable"))?
            .join("moonshine_voice/download.moonshine.ai/model/medium-streaming-en/quantized");
        if !model_path.exists() {
            return Err(eyre!(
                "Moonshine model is missing at {}. Run ./scripts/setup.sh",
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
                free_transcriber: *library.get(b"moonshine_free_transcriber\0")?,
                create_stream: *library.get(b"moonshine_create_stream\0")?,
                free_stream: *library.get(b"moonshine_free_stream\0")?,
                start_stream: *library.get(b"moonshine_start_stream\0")?,
                stop_stream: *library.get(b"moonshine_stop_stream\0")?,
                add_audio: *library.get(b"moonshine_transcribe_add_audio_to_stream\0")?,
                transcribe_stream: *library.get(b"moonshine_transcribe_stream\0")?,
                error_to_string: *library.get(b"moonshine_error_to_string\0")?,
            };
            let _native = write_native();
            let path = CString::new(model_path.to_string_lossy().as_bytes())?;
            let transcriber = load(
                path.as_ptr(),
                MEDIUM_STREAMING,
                ptr::null(),
                0,
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
    if let Ok(executable) = std::env::current_exe()
        && let Some(contents) = executable.parent().and_then(Path::parent)
    {
        let bundled = contents.join("Frameworks/libmoonshine.dylib");
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
            .join("site-packages/moonshine_voice/libmoonshine.dylib");
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
    use std::sync::{Arc, Barrier};

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
