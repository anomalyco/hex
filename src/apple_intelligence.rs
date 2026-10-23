//! The FoundationModels adapter: Apple's on-device language model behind a
//! small C bridge in `apple_intelligence.swift`.

use std::ffi::{CStr, CString, c_char};
use std::ptr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

unsafe extern "C" {
    fn hex_apple_intelligence_availability() -> i32;
    fn hex_apple_intelligence_context_size() -> isize;
    fn hex_apple_intelligence_notes(
        instructions: *const c_char,
        prompt: *const c_char,
        title_only: bool,
        timeout_ms: u64,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn hex_apple_intelligence_live_notes(
        instructions: *const c_char,
        prompt: *const c_char,
        timeout_ms: u64,
        error: *mut *mut c_char,
    ) -> *mut c_char;
    fn hex_apple_intelligence_free_string(string: *mut c_char);
}

const AVAILABILITY_TTL: Duration = Duration::from_secs(10);
const CONTEXT_WINDOW_MARKER: &str = "context-window:";
const REFUSAL_MARKER: &str = "refused:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Availability {
    Available,
    DeviceNotEligible,
    AppleIntelligenceNotEnabled,
    ModelNotReady,
    UnsupportedSystem,
    Unknown,
}

impl Availability {
    fn from_code(code: i32) -> Self {
        match code {
            0 => Self::Available,
            1 => Self::DeviceNotEligible,
            2 => Self::AppleIntelligenceNotEnabled,
            3 => Self::ModelNotReady,
            4 => Self::UnsupportedSystem,
            _ => Self::Unknown,
        }
    }

    pub fn is_available(self) -> bool {
        self == Self::Available
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Available => "Live notes, titles, and short summaries, written on this Mac",
            Self::DeviceNotEligible => "This Mac does not support Apple Intelligence",
            Self::AppleIntelligenceNotEnabled => {
                "Turn on Apple Intelligence in System Settings to use this"
            }
            Self::ModelNotReady => "The Apple Intelligence model is still downloading",
            Self::UnsupportedSystem => "Requires macOS 26 or later",
            Self::Unknown => "Apple Intelligence is unavailable",
        }
    }
}

/// The cached system availability; refreshed at most every ten seconds so
/// render paths can ask freely.
pub fn availability() -> Availability {
    static CACHE: Mutex<Option<(Instant, Availability)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some((checked, availability)) = *cache
        && checked.elapsed() < AVAILABILITY_TTL
    {
        return availability;
    }
    // SAFETY: A synchronous property read without arguments.
    let availability = Availability::from_code(unsafe { hex_apple_intelligence_availability() });
    *cache = Some((Instant::now(), availability));
    availability
}

/// The model's context window in tokens, or zero when unavailable.
pub fn context_tokens() -> usize {
    // SAFETY: A synchronous property read without arguments.
    usize::try_from(unsafe { hex_apple_intelligence_context_size() }).unwrap_or(0)
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
pub struct GeneratedNotes {
    pub title: String,
    #[serde(default)]
    pub summary: String,
}

/// One pass of live meeting notes: the current topic and at most three notes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, serde::Serialize)]
pub struct GeneratedLiveNotes {
    pub topic: String,
    #[serde(default)]
    pub notes: Vec<GeneratedLiveNote>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, serde::Serialize)]
pub struct GeneratedLiveNote {
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub time: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GenerationError {
    /// The prompt exceeded the model's context window.
    ContextWindow,
    /// The model refused the prompt or its guardrails blocked it.
    Refused,
    Failed(String),
}

impl std::fmt::Display for GenerationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ContextWindow => formatter.write_str("the transcript exceeded the model context"),
            Self::Refused => formatter.write_str("the model refused the transcript"),
            Self::Failed(message) => formatter.write_str(message),
        }
    }
}

/// Generates a meeting title and short summary with guided generation.
/// Blocks the calling thread for at most `timeout`; the bridge runs at
/// utility priority.
pub fn generate_notes(
    instructions: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<GeneratedNotes, GenerationError> {
    generate(instructions, prompt, false, timeout)
}

/// Generates only a meeting title, which needs far fewer output tokens.
pub fn generate_title(
    instructions: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<String, GenerationError> {
    generate(instructions, prompt, true, timeout).map(|notes| notes.title)
}

/// Generates one pass of live meeting notes over new transcript lines.
pub fn generate_live_notes(
    instructions: &str,
    prompt: &str,
    timeout: Duration,
) -> Result<GeneratedLiveNotes, GenerationError> {
    call(instructions, prompt, |instructions, prompt, error| {
        // SAFETY: Both strings outlive the call and the bridge writes only `error`.
        unsafe {
            hex_apple_intelligence_live_notes(
                instructions,
                prompt,
                timeout.as_millis().min(u64::MAX as u128) as u64,
                error,
            )
        }
    })
}

fn generate(
    instructions: &str,
    prompt: &str,
    title_only: bool,
    timeout: Duration,
) -> Result<GeneratedNotes, GenerationError> {
    call(instructions, prompt, |instructions, prompt, error| {
        // SAFETY: Both strings outlive the call and the bridge writes only `error`.
        unsafe {
            hex_apple_intelligence_notes(
                instructions,
                prompt,
                title_only,
                timeout.as_millis().min(u64::MAX as u128) as u64,
                error,
            )
        }
    })
}

/// Runs one bridge call and decodes its JSON result.
fn call<T: for<'de> Deserialize<'de>>(
    instructions: &str,
    prompt: &str,
    bridge: impl FnOnce(*const c_char, *const c_char, *mut *mut c_char) -> *mut c_char,
) -> Result<T, GenerationError> {
    let instructions =
        CString::new(instructions).map_err(|error| GenerationError::Failed(error.to_string()))?;
    let prompt = CString::new(prompt.replace('\0', ""))
        .map_err(|e| GenerationError::Failed(e.to_string()))?;
    let mut error = ptr::null_mut();
    let json = bridge(instructions.as_ptr(), prompt.as_ptr(), &mut error);
    if json.is_null() {
        let message = take_string(error).unwrap_or_else(|| "Apple Intelligence failed".into());
        return Err(classify_error(message));
    }
    let text = take_string(json).unwrap_or_default();
    serde_json::from_str(&text).map_err(|error| GenerationError::Failed(error.to_string()))
}

pub(crate) fn classify_error(message: String) -> GenerationError {
    if message.starts_with(CONTEXT_WINDOW_MARKER) || message.contains("context window") {
        GenerationError::ContextWindow
    } else if message.starts_with(REFUSAL_MARKER)
        || message.contains("refused to answer")
        || message.contains("guardrails were triggered")
    {
        GenerationError::Refused
    } else {
        GenerationError::Failed(message)
    }
}

fn take_string(string: *mut c_char) -> Option<String> {
    if string.is_null() {
        return None;
    }
    // SAFETY: The bridge returns NUL-terminated strings from `strdup`.
    let value = unsafe { CStr::from_ptr(string) }
        .to_string_lossy()
        .into_owned();
    // SAFETY: Ownership returns to the bridge allocator exactly once.
    unsafe { hex_apple_intelligence_free_string(string) };
    Some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_available_model_is_used() {
        assert!(Availability::from_code(0).is_available());
        for (code, expected) in [
            (1, Availability::DeviceNotEligible),
            (2, Availability::AppleIntelligenceNotEnabled),
            (3, Availability::ModelNotReady),
            (4, Availability::UnsupportedSystem),
            (99, Availability::Unknown),
            (-1, Availability::Unknown),
        ] {
            let availability = Availability::from_code(code);
            assert_eq!(availability, expected);
            assert!(!availability.is_available());
        }
        assert_eq!(
            Availability::AppleIntelligenceNotEnabled.description(),
            "Turn on Apple Intelligence in System Settings to use this"
        );
    }

    #[test]
    fn context_overflow_is_distinguished_from_other_failures() {
        assert_eq!(
            classify_error("context-window: 9000 tokens".into()),
            GenerationError::ContextWindow
        );
        assert_eq!(
            classify_error("Apple Intelligence timed out.".into()),
            GenerationError::Failed("Apple Intelligence timed out.".into())
        );
    }
}
