//! Shared identity for the global fallback and optional custom mode editors.
//!
//! Linux currently uses only the global target for text replacements. macOS
//! and Windows also use indexed targets for their behavior-complete Modes
//! panes.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModeTarget {
    Global,
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Mode(usize),
}

impl ModeTarget {
    pub(crate) fn id_fragment(self) -> String {
        match self {
            Self::Global => "global".into(),
            Self::Mode(index) => format!("mode-{index}"),
        }
    }
}
