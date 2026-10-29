//! The capsule's timings and springs in one place. The
//! app reads [`DEFAULT`]; the playground (`hex capsule-playground`) changes
//! them live in its own process and prints them back as the constants.

// Only the macOS playground reads much of this.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::sync::RwLock;

use rune::Spring;

pub(crate) use crate::app_settings::CapsuleFlow;

/// A flow's name on the playground's switch and in the clips.
pub(crate) fn flow_label(flow: CapsuleFlow) -> &'static str {
    match flow {
        CapsuleFlow::List => "A · List",
        CapsuleFlow::Toast => "B · Toast",
        CapsuleFlow::MenuBar => "C · Menu bar",
        CapsuleFlow::Cursor => "D · Cursor",
        CapsuleFlow::Notch => "E · Notch",
    }
}

/// How the finished capsule's top line reads, while Kit picks one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TopLine {
    /// A small check and "Copied" in quiet ink, then Paste and Inspect as
    /// icons beside ✕.
    Quiet,
    /// The check alone, no word; the icons and ✕ at the far end.
    Icons,
}

impl TopLine {
    pub(crate) const ALL: [TopLine; 2] = [TopLine::Quiet, TopLine::Icons];

    pub(crate) fn label(self) -> &'static str {
        match self {
            TopLine::Quiet => "A · Quiet label",
            TopLine::Icons => "B · Icons only",
        }
    }

    fn rust(self) -> &'static str {
        match self {
            TopLine::Quiet => "TopLine::Quiet",
            TopLine::Icons => "TopLine::Icons",
        }
    }
}

/// How the send-to list sets its key hints apart from its rows, while Kit
/// picks one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ListFooter {
    /// A band the list's full width, on its own quiet fill under a rule.
    Band,
    /// A rounded well inset like the rows, with no rule.
    Well,
}

impl ListFooter {
    pub(crate) const ALL: [ListFooter; 2] = [ListFooter::Band, ListFooter::Well];

    pub(crate) fn label(self) -> &'static str {
        match self {
            ListFooter::Band => "A · Band",
            ListFooter::Well => "B · Well",
        }
    }

    fn rust(self) -> &'static str {
        match self {
            ListFooter::Band => "ListFooter::Band",
            ListFooter::Well => "ListFooter::Well",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Tuning {
    /// The Undo window: a send, a rule's or a pick's, can be taken back
    /// for this long after it goes.
    pub(crate) undo_s: f64,
    /// The finished capsule leaves by itself after this long, unless its
    /// list is open.
    pub(crate) done_hold_s: f64,
    /// "Taken back" and errors that need nothing more leave sooner.
    pub(crate) brief_hold_s: f64,
    /// A recording's chips close by themselves this long after a tap.
    pub(crate) chips_open_s: f64,
    /// The walls, and every piece that stays and moves, on every change:
    /// about 0.3 s with a little bounce, as Dynamic Island's.
    pub(crate) island: Spring,
    /// The shell's fade and rise on entry; it leaves on 0.9 of it.
    pub(crate) island_scale: Spring,
    /// The selection highlight trailing a mode change.
    pub(crate) highlight: Spring,
    /// Content coming in: all of it at once, over the walls' move, never
    /// waiting for them or for what leaves.
    pub(crate) content_in: Spring,
    /// Content going, at the same moment, a little quicker.
    pub(crate) content_out: Spring,
    /// How blurred (pt) and how small, toward the capsule's middle, content
    /// is while it comes and goes. Rune keeps blur within 4 pt and scale
    /// within 0.9.
    pub(crate) content_blur: f32,
    pub(crate) content_scale: f32,
    pub(crate) top_line: TopLine,
    pub(crate) list_footer: ListFooter,
    /// The flow after a capture stops: `capture.capsule_flow` in the app,
    /// the playground's switch there.
    pub(crate) flow: CapsuleFlow,
}

pub(crate) const DEFAULT: Tuning = Tuning {
    undo_s: 5.0,
    done_hold_s: 25.0,
    brief_hold_s: 4.0,
    chips_open_s: 3.5,
    island: Spring::new(0.30, 0.12),
    island_scale: Spring::new(0.28, 0.10),
    highlight: Spring::new(0.25, 0.0),
    content_in: Spring::new(0.30, 0.10),
    content_out: Spring::new(0.24, 0.0),
    content_blur: 3.0,
    content_scale: 0.92,
    top_line: TopLine::Quiet,
    list_footer: ListFooter::Band,
    flow: CapsuleFlow::List,
};

static CURRENT: RwLock<Tuning> = RwLock::new(DEFAULT);

#[cfg(test)]
thread_local! {
    /// A test's flow, on its own thread only, so tests in parallel never
    /// see each other's.
    static TEST_FLOW: std::cell::Cell<Option<CapsuleFlow>> = const { std::cell::Cell::new(None) };
}

/// Runs `body` with `flow` in use on this thread.
#[cfg(test)]
pub(crate) fn with_flow<T>(flow: CapsuleFlow, body: impl FnOnce() -> T) -> T {
    let before = TEST_FLOW.replace(Some(flow));
    let out = body();
    TEST_FLOW.set(before);
    out
}

/// The values in use: [`DEFAULT`] unless the playground changed them.
pub(crate) fn get() -> Tuning {
    #[cfg(test)]
    if let Some(flow) = TEST_FLOW.get() {
        return Tuning {
            flow,
            ..*CURRENT.read().unwrap_or_else(|e| e.into_inner())
        };
    }
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

pub(crate) fn set(tuning: Tuning) {
    *CURRENT.write().unwrap_or_else(|e| e.into_inner()) = tuning;
}

impl Tuning {
    /// The values as `DEFAULT`'s Rust, to paste over it.
    pub(crate) fn to_rust(self) -> String {
        let spring = |s: Spring| format!("Spring::new({:.3}, {:.3})", s.duration, s.bounce);
        format!(
            "// src/rune_ui/capture_capsule/tuning.rs\n\
             pub(crate) const DEFAULT: Tuning = Tuning {{\n\
             \x20   undo_s: {:.2},\n\
             \x20   done_hold_s: {:.2},\n\
             \x20   brief_hold_s: {:.2},\n\
             \x20   chips_open_s: {:.2},\n\
             \x20   island: {},\n\
             \x20   island_scale: {},\n\
             \x20   highlight: {},\n\
             \x20   content_in: {},\n\
             \x20   content_out: {},\n\
             \x20   content_blur: {:.2},\n\
             \x20   content_scale: {:.3},\n\
             \x20   top_line: {},\n\
             \x20   list_footer: {},\n\
             \x20   flow: CapsuleFlow::{:?},\n\
             }};\n",
            self.undo_s,
            self.done_hold_s,
            self.brief_hold_s,
            self.chips_open_s,
            spring(self.island),
            spring(self.island_scale),
            spring(self.highlight),
            spring(self.content_in),
            spring(self.content_out),
            self.content_blur,
            self.content_scale,
            self.top_line.rust(),
            self.list_footer.rust(),
            self.flow,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_copied_values_name_every_field() {
        let rust = DEFAULT.to_rust();
        for field in [
            "undo_s: 5.00",
            "island: Spring::new(0.300, 0.120)",
            "content_in: Spring::new(0.300, 0.100)",
            "content_scale: 0.920",
            "top_line: TopLine::Quiet",
            "list_footer: ListFooter::Band",
            "flow: CapsuleFlow::List",
        ] {
            assert!(rust.contains(field), "{field} missing from\n{rust}");
        }
    }
}
