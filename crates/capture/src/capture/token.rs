//! Where a screen capture's ScreenCast portal restore token lives (Linux
//! Wayland). The host decides at each start whether it is one file for every
//! screen or one per output.

use std::path::PathBuf;

/// Where a screen capture's ScreenCast portal restore token lives.
#[derive(Clone, Debug, Default)]
pub enum PortalToken {
    /// Keep no token.
    #[default]
    None,
    /// One file for whichever screen the desktop picked.
    File(PathBuf),
    /// One file per output: the desktop is asked for `pointer`'s remembered
    /// output, and the token it answers with is saved for the output it
    /// actually records.
    PerOutput {
        /// The folder the token files live in, `<support>`.
        dir: PathBuf,
        /// The output under the pointer, whose remembered screen is asked for.
        pointer: String,
        /// The outputs' logical geometry, to name the recorded one when the
        /// desktop names no output.
        outputs: Vec<OutputBounds>,
    },
}

/// An output's connector name and logical geometry, as the host read it from
/// xdg-output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutputBounds {
    pub name: String,
    pub position: (i32, i32),
    pub size: (i32, i32),
}
