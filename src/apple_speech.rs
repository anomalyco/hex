use std::collections::{HashMap, HashSet};
use std::ffi::{CStr, CString, c_char, c_float, c_int, c_void};
use std::ptr;
use std::sync::{Mutex, OnceLock};

use color_eyre::eyre::{Result, eyre};
use serde::Deserialize;

use crate::transcription::{Transcript, TranscriptSegment};

unsafe extern "C" {
    fn hex_apple_speech_supported(locale: *const c_char) -> c_int;
    fn hex_apple_speech_ready(locale: *const c_char) -> c_int;
    fn hex_apple_speech_prepare(locale: *const c_char, error: *mut *mut c_char) -> *mut c_void;
    fn hex_apple_speech_transcribe(
        session: *mut c_void,
        samples: *const c_float,
        count: usize,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn hex_apple_speech_release(session: *mut c_void);
    fn hex_apple_speech_free_string(string: *mut c_char);
}

pub struct AppleSpeech {
    session: *mut c_void,
    selection: crate::transcription_models::TranscriptionSelection,
}

static SUPPORTED_LOCALES: OnceLock<Mutex<HashMap<String, bool>>> = OnceLock::new();
static READY_LOCALES: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

// The retained Swift actor is safe to move to the dedicated inference thread.
unsafe impl Send for AppleSpeech {}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeTranscript {
    text: String,
    segments: Vec<TranscriptSegment>,
}

impl AppleSpeech {
    pub fn is_supported(language: &str) -> bool {
        let mut locales = SUPPORTED_LOCALES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(supported) = locales.get(language) {
            return *supported;
        }
        let supported = with_locale(language, |locale| unsafe {
            hex_apple_speech_supported(locale) != 0
        })
        .unwrap_or(false);
        locales.insert(language.to_string(), supported);
        supported
    }

    pub fn is_ready(language: &str) -> bool {
        if READY_LOCALES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains(language)
        {
            return true;
        }
        let ready = with_locale(language, |locale| unsafe {
            hex_apple_speech_ready(locale) != 0
        })
        .unwrap_or(false);
        if ready {
            READY_LOCALES
                .get_or_init(Default::default)
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .insert(language.to_string());
        }
        ready
    }

    pub fn load(selection: &crate::transcription_models::TranscriptionSelection) -> Result<Self> {
        let locale = CString::new(selection.language.as_str())?;
        let mut error = ptr::null_mut();
        let session = unsafe { hex_apple_speech_prepare(locale.as_ptr(), &mut error) };
        if session.is_null() {
            return Err(take_error(error));
        }
        READY_LOCALES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .insert(selection.language.clone());
        Ok(Self {
            session,
            selection: selection.clone(),
        })
    }

    pub fn matches_selection(
        &self,
        selection: &crate::transcription_models::TranscriptionSelection,
    ) -> bool {
        &self.selection == selection
    }

    pub fn transcribe(&mut self, samples: &[f32]) -> Result<Transcript> {
        let mut error = ptr::null_mut();
        let json = unsafe {
            hex_apple_speech_transcribe(self.session, samples.as_ptr(), samples.len(), &mut error)
        };
        if json.is_null() {
            return Err(take_error(error));
        }
        let bytes = unsafe { CStr::from_ptr(json) }.to_bytes();
        let decoded = serde_json::from_slice::<BridgeTranscript>(bytes)
            .map(|result| Transcript {
                text: result.text,
                segments: result.segments,
            })
            .map_err(Into::into);
        unsafe { hex_apple_speech_free_string(json) };
        decoded
    }
}

impl Drop for AppleSpeech {
    fn drop(&mut self) {
        unsafe { hex_apple_speech_release(self.session) };
    }
}

fn with_locale<T>(language: &str, operation: impl FnOnce(*const c_char) -> T) -> Result<T> {
    let locale = CString::new(language)?;
    Ok(operation(locale.as_ptr()))
}

fn take_error(error: *mut c_char) -> color_eyre::Report {
    if error.is_null() {
        return eyre!("Apple Speech failed without an error");
    }
    let message = unsafe { CStr::from_ptr(error) }
        .to_string_lossy()
        .into_owned();
    unsafe { hex_apple_speech_free_string(error) };
    eyre!(message)
}
