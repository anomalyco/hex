//! What Hex's window asks about, and what the person answers.
//!
//! Both consents (transcribing audio, and starting a microphone recording)
//! name the recipient the grant is scored to. A browser page is named by the
//! `Origin` its browser sent; a native process is named by the code signing
//! identity resolved from its Unix-socket peer audit token, never by an app
//! name or a path it claims.

use std::time::Duration;

/// What the person is being asked to allow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Intent {
    /// Transcribe audio on this Mac and return one transcript.
    Transcribe,
    /// Start one microphone recording and return its transcript.
    Record,
}

/// Who receives what the grant produces, as Hex can verify it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Recipient {
    /// A browser page: the `Origin` header of a listed Labs origin.
    Web {
        origin: String,
        /// The page's own short plain name, when it sent one.
        client_name: Option<String>,
    },
    /// A native process on Hex's owner-only Unix socket: the signing
    /// identifier and team `Security.framework` reported for its audit token.
    Native {
        identifier: String,
        cdhash: String,
        team: Option<String>,
    },
}

impl Recipient {
    /// The one line the dialog shows beside "For".
    pub(crate) fn label(&self) -> String {
        match self {
            Self::Web { origin, .. } => origin.clone(),
            Self::Native { identifier, .. } => identifier.clone(),
        }
    }

    /// The page's own name, which is only a claim.
    pub(crate) fn claimed_name(&self) -> Option<&str> {
        match self {
            Self::Web { client_name, .. } => client_name.as_deref(),
            Self::Native { .. } => None,
        }
    }
}

/// One ask, as the window shows it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Prompt {
    /// Assigned by the consent gate; the window answers with it.
    pub(crate) id: u64,
    pub(crate) intent: Intent,
    pub(crate) recipient: Recipient,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Answer {
    /// The person refused: nothing is granted.
    Deny,
    /// Exactly one operation, with no saved permission.
    Once,
    /// A saved permission, reused across restarts until revoked.
    Always,
    /// Nobody answered within the wait.
    Timeout,
    /// No window can ask (headless service, or the window is gone).
    Unavailable,
}

/// The window's side of the ask. `ask` shows the prompt and blocks until the
/// person answers or `wait` passes.
pub(crate) trait ConsentGate: Send + Sync {
    fn ask(&self, prompt: &Prompt, wait: Duration) -> Answer;

    /// Ends the ask shown now, answered [`Answer::Unavailable`], unless
    /// `keep` says it may stay (Labs › Connected apps no longer allows its
    /// recipient).
    fn withdraw(&self, _keep: &dyn Fn(&Prompt) -> bool) {}
}
