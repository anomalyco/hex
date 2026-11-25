//! Meetings › Computer audio's app and output lists, read off the UI thread.
//!
//! Listing runs `pactl` and reads the PipeWire graph on Linux, and Core
//! Audio's process list or COM's audio sessions elsewhere, so the window
//! starts one read when the Settings view shows and adopts its answer in a
//! poll. Until the first answer the row offers all computer audio alone.

use std::sync::mpsc::{Receiver, TryRecvError, channel};

use super::super::{Pane, Ui};
use super::View;
use crate::desktop::i18n::tr;
use crate::meeting::audio_source::{self, SourceChoice, SourceEntry};

/// The sources a meeting's system track can record, as of the last read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::rune_ui) struct Sources {
    /// Output devices, the default first.
    pub(in crate::rune_ui) outputs: Vec<SourceEntry>,
    /// Applications playing now.
    pub(in crate::rune_ui) apps: Vec<SourceEntry>,
}

/// The row's last read and the one in flight.
#[derive(Default)]
pub(in crate::rune_ui) struct Listing {
    /// The last finished read, if any.
    pub(in crate::rune_ui) sources: Option<Sources>,
    read: Option<Receiver<Sources>>,
    /// The poll clock the last read started at.
    read_at: f64,
}

impl Listing {
    /// The sources the row offers now.
    pub(in crate::rune_ui) fn sources(&self) -> Option<&Sources> {
        self.sources.as_ref()
    }
}

/// The Computer audio picker's entries: all computer audio, then the listed
/// outputs and apps, then a saved choice the list doesn't have with why. A
/// choice matches its listed source by id, so a saved app whose display name
/// changed is that app rather than a second "not playing now" entry.
pub(in crate::rune_ui) fn choices(
    choice: &SourceChoice,
    listed: Option<&Sources>,
) -> Vec<(SourceChoice, String, Option<&'static str>)> {
    let mut options = vec![(SourceChoice::All, tr("All computer audio").to_owned(), None)];
    if let Some(listed) = listed {
        options.extend(listed.outputs.iter().map(|output| {
            (
                SourceChoice::Output {
                    id: output.id.clone(),
                    name: output.name.clone(),
                },
                output.name.clone(),
                None,
            )
        }));
        options.extend(listed.apps.iter().map(|app| {
            (
                SourceChoice::App {
                    id: app.id.clone(),
                    name: app.name.clone(),
                },
                app.name.clone(),
                None,
            )
        }));
    }
    if !options
        .iter()
        .any(|(option, ..)| option.same_source(choice))
        && let Some(hint) = choice.missing_hint()
    {
        options.insert(1, (choice.clone(), choice.name().to_owned(), Some(hint)));
    }
    options
}

/// Starts a read unless one is in flight. A stale source list is kept while
/// the next answer arrives, so the row never flickers.
pub(in crate::rune_ui) fn refresh(ui: &mut Ui) {
    if !ui.live() || ui.meeting_audio.read.is_some() {
        return;
    }
    let (sender, receiver) = channel();
    let waker = ui.modes.waker.clone();
    let spawned = std::thread::Builder::new()
        .name("hex-meeting-audio".into())
        .spawn(move || {
            let _ = sender.send(Sources {
                outputs: audio_source::output_devices(),
                apps: audio_source::playing_apps(),
            });
            if let Some(waker) = &waker {
                waker();
            }
        });
    if spawned.is_ok() {
        ui.meeting_audio.read = Some(receiver);
    }
}

/// Adopts a finished read and starts the next while Meetings › Settings ›
/// Computer audio shows; true when the adopted list changed what shows.
pub(in crate::rune_ui) fn poll(ui: &mut Ui, clock: f64) -> bool {
    let changed = adopt(ui);
    if ui.pane == Pane::Meetings
        && ui.meetings.view == View::Settings
        && ui.settings.labs.meeting_audio_source
        && clock >= ui.meeting_audio.read_at + 3.0
    {
        refresh(ui);
        ui.meeting_audio.read_at = clock;
    }
    changed
}

fn adopt(ui: &mut Ui) -> bool {
    let Some(read) = &ui.meeting_audio.read else {
        return false;
    };
    match read.try_recv() {
        Ok(sources) => {
            let changed = ui.meeting_audio.sources.as_ref() != Some(&sources);
            ui.meeting_audio.sources = Some(sources);
            ui.meeting_audio.read = None;
            changed
        }
        Err(TryRecvError::Empty) => false,
        Err(TryRecvError::Disconnected) => {
            ui.meeting_audio.read = None;
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A saved app whose display name changed matches the listed source by
    /// id, so the picker offers it once instead of adding a second "not
    /// playing now" row.
    #[test]
    fn a_renamed_saved_app_is_listed_once() {
        let saved = SourceChoice::App {
            id: "google-chrome".into(),
            name: "Google Chrome".into(),
        };
        let listed = Sources {
            outputs: Vec::new(),
            apps: vec![SourceEntry {
                id: "google-chrome".into(),
                name: "Chrome".into(),
            }],
        };
        let options = choices(&saved, Some(&listed));
        assert_eq!(options.len(), 2, "all audio and the listed app");
        assert_eq!(
            options[1].0,
            SourceChoice::App {
                id: "google-chrome".into(),
                name: "Chrome".into(),
            }
        );
        assert_eq!(options[1].2, None, "no missing row for a listed app");

        // A saved app the list doesn't have keeps its own row and hint, even
        // when another app shares its display name.
        let missing = SourceChoice::App {
            id: "slack".into(),
            name: "Google Chrome".into(),
        };
        let options = choices(&missing, Some(&listed));
        assert_eq!(options.len(), 3);
        assert!(options[1].0.same_source(&missing));
        assert_eq!(options[1].2, Some("not playing now"));
    }
}
