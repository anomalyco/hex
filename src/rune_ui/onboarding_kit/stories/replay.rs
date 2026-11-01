//! The real first run's replay strip, in test and debug builds only
//! (`hex_dev_onboarding`): restart it, go back one beat, play another joke,
//! and fake a slower download. Every build logs how long the download
//! showed for.

use rune_kit::{Button, ButtonVariant, ControlSize};

use super::super::live;
use super::*;

/// Whether this build has the strip, and the jokes that swear.
const ON: bool = cfg!(hex_dev_onboarding);

/// How fast the download shows, at most.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Speed {
    /// As fast as Hex downloads it.
    #[default]
    Real,
    Mbps100,
    Mbps25,
}

impl Speed {
    fn next(self) -> Self {
        match self {
            Speed::Real => Speed::Mbps100,
            Speed::Mbps100 => Speed::Mbps25,
            Speed::Mbps25 => Speed::Real,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Speed::Real => "Real speed",
            Speed::Mbps100 => "100 Mbps",
            Speed::Mbps25 => "25 Mbps",
        }
    }

    fn bytes_per_sec(self) -> Option<f64> {
        match self {
            Speed::Real => None,
            Speed::Mbps100 => Some(100e6 / 8.0),
            Speed::Mbps25 => Some(25e6 / 8.0),
        }
    }
}

/// The download as the first run shows it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Replay {
    speed: Speed,
    /// When the shown download started.
    from: Option<f64>,
    /// How long it showed for, once it finished.
    took: Option<f64>,
}

impl Replay {
    /// Holds Hex's real `download` to the chosen speed, and logs how long
    /// it showed for once it finishes. A model already in place shows
    /// nothing.
    pub(crate) fn show(&mut self, download: &mut Download, now: f64) {
        if self.from.is_none() && download.done() {
            return;
        }
        let from = *self.from.get_or_insert(now);
        if let Some(rate) = self.speed.bytes_per_sec() {
            download.downloaded = download.downloaded.min((rate * (now - from)) as u64);
        }
        if download.done() && self.took.is_none() {
            let seconds = now - from;
            self.took = Some(seconds);
            tracing::info!(
                seconds,
                megabytes = download.total / 1_000_000,
                speed = self.speed.label(),
                "first run download shown"
            );
        }
    }
}

impl Onboard {
    /// The first run from its opener, the pointer clicking the Launch
    /// Room open; a faked speed downloads the model again as it plays.
    pub(crate) fn restart(&mut self) {
        if self.replay.speed != Speed::Real {
            self.replay.from = Some(self.now);
            self.replay.took = None;
            self.had_model = false;
            self.closed = 0.0;
            self.closed_from = None;
            if let Some(download) = self.download.as_mut() {
                download.downloaded = 0;
            }
        }
        self.opener();
    }

    /// The start of the beat before this one.
    pub(crate) fn back_one_beat(&mut self) {
        match self.step {
            Step::YourTurn | Step::Hearing | Step::Heard => self.without_hex(),
            Step::Microphone
            | Step::Accessibility
            | Step::Waiting
            | Step::Try
            | Step::TryHearing
            | Step::TryHeard => self.with_hex(),
            Step::Ready => self.jump_to_try(),
            Step::Pick | Step::Started | Step::Parked | Step::Done => self.opener(),
        }
    }

    /// The next bundled joke, from the opener.
    pub(crate) fn next_joke(&mut self) {
        Script::choose_next();
        self.scene = typing::Scene::build(&self.timing);
        self.take = Script::chosen().take();
        self.opener();
    }

    pub(crate) fn next_speed(&mut self) {
        self.replay.speed = self.replay.speed.next();
    }

    fn opener(&mut self) {
        self.jump_to_demo();
        self.auto_due_at = Some(self.now + WATCH_AFTER);
    }

    fn without_hex(&mut self) {
        self.jump_to_demo();
        self.start_scene();
    }

    /// `"With Hex"` from the top, `"Without Hex"` just behind it.
    fn with_hex(&mut self) {
        self.jump_to_demo();
        let now = self.now;
        self.typing_from = Some(now - self.scene.duration() - COMEDY_HOLD);
        self.step = Step::YourTurn;
        self.voice_from = Some(now);
        self.hud_from = Some(now);
        self.sfx.on_rewind(now);
        self.auto_due_at = Some(now + versus::BEFORE_KEY);
    }
}

/// The strip in the window's corner, on the real first run.
pub(crate) fn strip(p: Paint, s: &Onboard) -> Option<El> {
    if !ON || s.live.is_none() {
        return None;
    }
    let button = |key: &str, label: String, f: fn(&mut Onboard)| {
        Button::new(label)
            .variant(ButtonVariant::GhostMuted)
            .control_size(ControlSize::Small)
            .on_press(live::on_ui(f))
            .build(p.k)
            .key(key.to_string())
    };
    let took = s.replay.took.zip(s.download).map(|(secs, download)| {
        text(format!("{} MB in {secs:.1} s", download.total / 1_000_000))
            .key("took")
            .font(p.k.ty.small)
            .color(p.faint())
    });
    Some(
        row()
            .key("replay")
            .pin(Some(12.0), None, None, Some(12.0))
            .gap(2.0)
            .center_y()
            .child(button("restart", "Restart".into(), Onboard::restart))
            .child(button("back", "Back".into(), Onboard::back_one_beat))
            .child(button(
                "joke",
                format!("Joke: {}", Script::chosen_name()),
                Onboard::next_joke,
            ))
            .child(button(
                "speed",
                s.replay.speed.label().into(),
                Onboard::next_speed,
            ))
            .children(took),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_faked_speed_holds_the_download_back_and_times_it() {
        let mut replay = Replay {
            speed: Speed::Mbps100,
            ..Replay::default()
        };
        let total = 731_000_000;
        let mut download = Download {
            language: Language::English,
            downloaded: 1_000,
            total,
        };
        replay.show(&mut download, 10.0);
        assert_eq!(download.downloaded, 0);
        let mut download = Download {
            downloaded: total,
            ..download
        };
        replay.show(&mut download, 40.0);
        assert_eq!(download.downloaded, 375_000_000);
        assert_eq!(replay.took, None);
        let mut download = Download {
            downloaded: total,
            ..download
        };
        replay.show(&mut download, 70.0);
        assert!(download.done());
        assert_eq!(replay.took, Some(60.0));
    }

    #[test]
    fn a_model_already_in_place_shows_no_download() {
        let mut replay = Replay::default();
        let mut download = Download {
            language: Language::English,
            downloaded: 5,
            total: 5,
        };
        replay.show(&mut download, 1.0);
        assert_eq!(replay.took, None);
    }

    #[test]
    fn back_from_with_hex_plays_without_hex_from_the_top() {
        let mut s = Onboard::new(Flow::FirstRun);
        s.now = 5.0;
        s.with_hex();
        assert_eq!(s.step, Step::YourTurn);
        s.back_one_beat();
        assert_eq!(s.step, Step::Parked);
        assert_eq!(s.typing_from, Some(5.0 + typing::INTRO));
        s.back_one_beat();
        assert_eq!(s.typing_from, None);
        assert_eq!(s.auto_due_at, Some(5.0 + WATCH_AFTER));
    }
}
