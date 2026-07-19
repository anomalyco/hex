use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_float, c_int, c_void};
use std::ptr;
use std::sync::{Mutex, OnceLock};

use crate::transcription::Transcript;
use color_eyre::eyre::{Result, eyre};

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
static READY_LOCALES: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

// The retained Swift actor is safe to move to the dedicated inference thread.
unsafe impl Send for AppleSpeech {}

impl AppleSpeech {
    pub fn support_status(language: &str) -> Option<bool> {
        let mut locales = SUPPORTED_LOCALES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(supported) = locales.get(language) {
            return Some(*supported);
        }
        let supported = with_locale(language, |locale| unsafe {
            hex_apple_speech_supported(locale)
        })
        .unwrap_or(-1);
        if supported < 0 {
            return None;
        }
        let supported = supported != 0;
        locales.insert(language.to_string(), supported);
        Some(supported)
    }

    pub fn readiness_status(language: &str) -> Option<bool> {
        if READY_LOCALES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .contains_key(language)
        {
            return Some(true);
        }
        let ready =
            with_locale(language, |locale| unsafe { hex_apple_speech_ready(locale) }).unwrap_or(-1);
        if ready < 0 {
            return None;
        }
        let ready = ready != 0;
        if ready {
            return Some(true);
        }
        Some(false)
    }

    pub fn is_ready(language: &str) -> bool {
        Self::readiness_status(language).unwrap_or(false)
    }

    pub fn load(selection: &crate::transcription_models::TranscriptionSelection) -> Result<Self> {
        let mut error = ptr::null_mut();
        let session = with_locale(&selection.language, |locale| unsafe {
            hex_apple_speech_prepare(locale, &mut error)
        })?;
        if session.is_null() {
            return Err(take_error(error));
        }
        READY_LOCALES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(selection.language.clone())
            .and_modify(|references| *references += 1)
            .or_insert(1);
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
        let decoded = serde_json::from_slice::<Transcript>(bytes).map_err(Into::into);
        unsafe { hex_apple_speech_free_string(json) };
        decoded
    }
}

impl Drop for AppleSpeech {
    fn drop(&mut self) {
        unsafe { hex_apple_speech_release(self.session) };
        let mut ready = READY_LOCALES
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if let Some(references) = ready.get_mut(&self.selection.language) {
            if *references > 1 {
                *references -= 1;
            } else {
                ready.remove(&self.selection.language);
            }
        }
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
