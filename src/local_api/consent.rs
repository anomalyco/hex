//! What Hex's window asks about, and what the person answers.
//!
//! Both consents (transcribing audio, and starting a microphone recording)
//! name the recipient the grant is scored to. A browser page is named by the
//! `Origin` its browser sent; a native process is named by what the OS
//! verified about the peer (on macOS, the code signing identity resolved from
//! its Unix-socket peer audit token), never by an app name or a path it
//! claims. A recipient's name and icon are display data only: grants, their
//! persisted scope and their in-memory keys ignore both.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// What the person is being asked to allow.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Intent {
    /// Transcribe audio on this Mac and return one transcript.
    Transcribe,
    /// Start one microphone recording and return its transcript.
    Record,
}

/// A small square RGBA8 image Hex derived itself, for display only.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ConsentIcon {
    size: u32,
    pixels: Arc<[u8]>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl ConsentIcon {
    /// The largest edge, in pixels, an icon may have.
    pub(crate) const MAX_SIZE: u32 = 128;

    /// `size` by `size` straight RGBA8 pixels, row by row; `None` unless
    /// `0 < size <= MAX_SIZE` and `pixels` holds exactly that many.
    pub(crate) fn new(size: u32, pixels: Arc<[u8]>) -> Option<Self> {
        let fits =
            (1..=Self::MAX_SIZE).contains(&size) && pixels.len() == (size * size * 4) as usize;
        fits.then_some(Self { size, pixels })
    }

    pub(crate) fn size(&self) -> u32 {
        self.size
    }

    pub(crate) fn pixels(&self) -> &[u8] {
        &self.pixels
    }
}

/// How the OS vouched for a native requester. Part of the grant's identity.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Verification {
    /// A valid macOS code signature, with its Team ID when it has one.
    MacSignature { team: Option<String> },
    /// An Authenticode signature chaining to a trusted root; `publisher` is
    /// the signer certificate's subject CN.
    WindowsSignature { publisher: String },
    /// A Linux sandbox app id, read from sandbox metadata the kernel shows
    /// for the peer, not from anything the peer sent.
    Sandbox { kind: SandboxKind, app_id: String },
    /// The executable the kernel/OS reports for the peer process (Linux, or
    /// an unsigned Windows program).
    Executable { path: PathBuf },
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum SandboxKind {
    Flatpak,
    Snap,
}

/// Who receives what the grant produces, as Hex can verify it.
///
/// Derived equality compares every field, display data included. Grant
/// lookup goes through `permissions::Scope` and `record`'s recipient key,
/// which ignore `name`, `icon` and `client_name`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Recipient {
    /// A browser page: the `Origin` header of a listed Labs origin.
    Web {
        origin: String,
        /// The page's own short plain name, when it sent one.
        client_name: Option<String>,
        /// The page's favicon. Display only, and only a claim.
        icon: Option<ConsentIcon>,
    },
    /// A native process on Hex's owner-only local socket.
    Native {
        /// The stable grant key: the signing identifier on macOS.
        identifier: String,
        /// The verified code-directory hash on macOS.
        cdhash: String,
        verification: Verification,
        /// A display name Hex derived from verified data (bundle, version
        /// info, `.desktop` file), never one the client claimed.
        name: Option<String>,
        /// An icon Hex derived from the same verified data. Display only.
        icon: Option<ConsentIcon>,
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

    /// The name Hex derived for a native app, else its identifier; a page's
    /// origin (never its claimed name).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn display_name(&self) -> &str {
        match self {
            Self::Web { origin, .. } => origin,
            Self::Native {
                name, identifier, ..
            } => name.as_deref().unwrap_or(identifier),
        }
    }

    /// How the OS vouched for a native requester; `None` for a page.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn verification(&self) -> Option<&Verification> {
        match self {
            Self::Web { .. } => None,
            Self::Native { verification, .. } => Some(verification),
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn icon(&self) -> Option<&ConsentIcon> {
        match self {
            Self::Web { icon, .. } | Self::Native { icon, .. } => icon.as_ref(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_icon_is_a_small_square_of_rgba_pixels() {
        let icon = ConsentIcon::new(2, Arc::from([7u8; 16])).unwrap();
        assert_eq!((icon.size(), icon.pixels().len()), (2, 16));
        assert!(ConsentIcon::new(2, Arc::from([0u8; 12])).is_none());
        assert!(ConsentIcon::new(0, Arc::from([])).is_none());
        let big = ConsentIcon::MAX_SIZE + 1;
        let pixels: Arc<[u8]> = vec![0; (big * big * 4) as usize].into();
        assert!(ConsentIcon::new(big, pixels).is_none());
    }

    #[test]
    fn a_native_display_name_falls_back_to_its_identifier() {
        let mut app = Recipient::Native {
            identifier: "com.example.notes".into(),
            cdhash: "fixture-digest".into(),
            verification: Verification::MacSignature { team: None },
            name: None,
            icon: None,
        };
        assert_eq!(app.display_name(), "com.example.notes");
        if let Recipient::Native { name, .. } = &mut app {
            *name = Some("Notes".into());
        }
        assert_eq!(app.display_name(), "Notes");
        let page = Recipient::Web {
            origin: "https://notes.example".into(),
            client_name: Some("Notes".into()),
            icon: None,
        };
        assert_eq!(page.display_name(), "https://notes.example");
        assert_eq!(page.verification(), None);
        assert_eq!(page.icon(), None);
    }
}
