use crate::transcription::SpeechBackend;
use crate::transcription::models::{
    TranscriptionModelId, TranscriptionSelection, definition, validate,
};

use super::Model;

/// The engine worker's model slot. Every state carries exactly the metadata
/// valid for it, so a loaded model, a parked selection, a reload failure, and
/// streaming capability can never disagree.
pub(super) enum ModelSlot {
    /// No model is loaded and none is parked to reload.
    #[allow(dead_code)]
    Empty,
    /// A model is loaded on the worker thread.
    Loaded { model: Box<Model>, streams: bool },
    /// Unloaded for idleness (Labs › Free memory when idle) or about to reload
    /// in place: still counts as held so callers never load a second copy, and
    /// reloads before the next command that needs it.
    Parked {
        selection: TranscriptionSelection,
        streams: bool,
    },
    /// Reloading `selection` failed: retried on the next command that needs
    /// it, never claims to stream, and does not count as held.
    ReloadFailed {
        selection: TranscriptionSelection,
        error: String,
    },
}

impl ModelSlot {
    pub(super) fn loaded(model: Model) -> Self {
        let streams = model.stream_options().is_some();
        Self::Loaded {
            model: Box::new(model),
            streams,
        }
    }

    pub(super) fn parked(selection: TranscriptionSelection) -> Self {
        let streams = validate(&selection).is_ok_and(|model| model.streams);
        Self::Parked { selection, streams }
    }

    pub(super) fn model(&self) -> Option<&Model> {
        match self {
            Self::Loaded { model, .. } => Some(model.as_ref()),
            Self::Empty | Self::Parked { .. } | Self::ReloadFailed { .. } => None,
        }
    }

    pub(super) fn model_mut(&mut self) -> Option<&mut Model> {
        match self {
            Self::Loaded { model, .. } => Some(model.as_mut()),
            Self::Empty | Self::Parked { .. } | Self::ReloadFailed { .. } => None,
        }
    }

    /// Re-reads streaming capability from the loaded model after an in-place
    /// activation, model switch, or stream refresh.
    pub(super) fn sync_streams(&mut self) {
        if let Self::Loaded { model, streams } = self {
            *streams = model.stream_options().is_some();
        }
    }

    /// Unloads a loaded GGUF model, freeing its memory while remembering its
    /// selection and streaming capability to reload. Apple Speech stays loaded
    /// because it holds no model weights of Hex's.
    pub(super) fn park(&mut self) -> Option<TranscriptionSelection> {
        let Self::Loaded { model, streams } = self else {
            return None;
        };
        let selection = model.gguf_selection()?.clone();
        let streams = *streams;
        *self = Self::Parked {
            selection: selection.clone(),
            streams,
        };
        Some(selection)
    }

    /// The selection to reload when the slot has no loaded model.
    pub(super) fn reload_target(&self) -> Option<TranscriptionSelection> {
        match self {
            Self::Parked { selection, .. } | Self::ReloadFailed { selection, .. } => {
                Some(selection.clone())
            }
            Self::Empty | Self::Loaded { .. } => None,
        }
    }

    pub(super) fn reload_error(&self) -> Option<&str> {
        match self {
            Self::ReloadFailed { error, .. } => Some(error.as_str()),
            Self::Empty | Self::Loaded { .. } | Self::Parked { .. } => None,
        }
    }

    pub(super) fn snapshot(&self) -> HeldState {
        match self {
            Self::Empty => HeldState::Empty,
            Self::Loaded { model, streams } => {
                #[cfg(not(target_os = "macos"))]
                let backend = Some(model.backend().clone());
                #[cfg(target_os = "macos")]
                let backend = None;
                HeldState::Loaded {
                    selection: model.gguf_selection().cloned(),
                    backend,
                    streams: *streams,
                }
            }
            Self::Parked { selection, streams } => HeldState::Parked {
                selection: selection.clone(),
                streams: *streams,
            },
            Self::ReloadFailed { selection, .. } => HeldState::ReloadFailed {
                selection: selection.clone(),
            },
        }
    }
}

/// What callers outside the engine thread can observe about the worker's
/// model slot: one snapshot so selection, backend, and streaming capability
/// never drift apart.
#[derive(Clone, Default)]
pub(super) enum HeldState {
    #[default]
    Empty,
    Loaded {
        selection: Option<TranscriptionSelection>,
        backend: Option<SpeechBackend>,
        streams: bool,
    },
    /// Unloaded for idleness (Labs › Free memory when idle): still counts as
    /// held so callers never load a second copy, and reports no live device.
    Parked {
        selection: TranscriptionSelection,
        streams: bool,
    },
    /// Reloading `selection` failed: never counts as held and cannot stream.
    ReloadFailed {
        #[allow(dead_code)]
        selection: TranscriptionSelection,
    },
}

impl HeldState {
    pub(super) fn streams(&self) -> bool {
        match self {
            Self::Loaded { streams, .. } | Self::Parked { streams, .. } => *streams,
            Self::Empty | Self::ReloadFailed { .. } => false,
        }
    }

    pub(super) fn holds(&self, selection: &TranscriptionSelection) -> bool {
        match self {
            Self::Loaded {
                selection: Some(held),
                ..
            }
            | Self::Parked {
                selection: held, ..
            } => held == selection,
            Self::Empty
            | Self::Loaded {
                selection: None, ..
            }
            | Self::ReloadFailed { .. } => false,
        }
    }

    pub(super) fn selection(&self) -> Option<TranscriptionSelection> {
        match self {
            Self::Loaded { selection, .. } => selection.clone(),
            Self::Parked { selection, .. } => Some(selection.clone()),
            Self::Empty | Self::ReloadFailed { .. } => None,
        }
    }

    pub(super) fn backend(&self) -> Option<SpeechBackend> {
        match self {
            Self::Loaded { backend, .. } => backend.clone(),
            Self::Empty | Self::Parked { .. } | Self::ReloadFailed { .. } => None,
        }
    }

    /// Why a live stream cannot open for a capture, when it can't: the
    /// model's own definition doesn't stream, or it streams in principle but
    /// not on the device it loaded on (a CPU, or a GPU whose stream probe
    /// failed). `None` when a stream can open.
    pub(super) fn stream_refusal(&self) -> Option<&'static str> {
        if self.streams() {
            return None;
        }
        match self.selection() {
            Some(selection) => {
                // An imported model's catalog entry says it can't stream, but
                // its GGUF is only checked when it loads; blame the device.
                if !matches!(selection.model, TranscriptionModelId::Custom(_))
                    && !definition(selection.model).streams
                {
                    Some("the selected speech model doesn't stream")
                } else {
                    Some(
                        "the speech model isn't streaming on this device (a CPU, or a GPU that could not start streaming)",
                    )
                }
            }
            None => Some("no streaming speech model is loaded"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parked_counts_as_held_while_reload_failed_does_not_and_clears_streaming() {
        let streaming = TranscriptionSelection {
            model: TranscriptionModelId::ParakeetUnifiedEnglish,
            language: "en".into(),
        };
        let parked = ModelSlot::parked(streaming.clone()).snapshot();
        assert!(parked.holds(&streaming));
        assert_eq!(parked.selection(), Some(streaming.clone()));
        assert!(parked.backend().is_none());
        assert!(parked.streams());
        assert_eq!(parked.stream_refusal(), None);

        let failed = ModelSlot::ReloadFailed {
            selection: streaming.clone(),
            error: "boom".into(),
        }
        .snapshot();
        assert!(
            !failed.holds(&streaming),
            "a failed reload must not claim to hold the model"
        );
        assert_eq!(failed.selection(), None);
        assert!(failed.backend().is_none());
        assert!(
            !failed.streams(),
            "a failed reload must never keep stale streaming capability"
        );
        assert_eq!(
            failed.stream_refusal(),
            Some("no streaming speech model is loaded")
        );
    }

    #[test]
    fn non_streaming_parked_model_refuses_streams_naming_the_model() {
        let non_streaming = TranscriptionSelection {
            model: TranscriptionModelId::ParakeetV3,
            language: "en".into(),
        };
        let parked = ModelSlot::parked(non_streaming.clone()).snapshot();
        assert!(parked.holds(&non_streaming));
        assert!(!parked.streams());
        assert_eq!(
            parked.stream_refusal(),
            Some("the selected speech model doesn't stream")
        );
    }
}
