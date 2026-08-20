//! Shared identity for the global fallback and optional custom mode editors.
//!
//! All three roots use the global and indexed targets for their concrete Modes
//! panes; later processing cards reuse the same identity without coupling it
//! to a platform settings schema.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ModeTarget {
    Global,
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
