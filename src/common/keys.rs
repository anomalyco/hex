//! Platform-neutral key and modifier types for the command taxonomy.
//! Actions describe keystrokes with these; each platform's executor maps
//! them to its own synthesis API (CoreGraphics on macOS, XTest on X11).
//! `COMMAND` is the platform primary shortcut modifier: ⌘ on macOS and
//! Ctrl elsewhere, so one taxonomy drives the same app shortcuts
//! everywhere; `OPTION` is ⌥/Alt.
//!
//! This is a shared vocabulary: each shell constructs only the variants
//! its built-ins use, so per-platform dead-code analysis stays quiet here.
#![allow(dead_code)]

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Character(char),
    Home,
    End,
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const COMMAND: Self = Self(1 << 0);
    pub const SHIFT: Self = Self(1 << 1);
    pub const OPTION: Self = Self(1 << 2);
    pub const CONTROL: Self = Self(1 << 3);

    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}
