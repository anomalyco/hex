//! Track warnings that outlive the recording's live state.

use serde::{Deserialize, Serialize};

/// A problem with one captured track that the user should know about.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackWarning {
    /// The microphone stream delivered only exact zeros, as a call app
    /// holding the device with voice processing does (#1518).
    DigitalSilence,
}
