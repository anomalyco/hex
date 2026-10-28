//! Pause a browser that is audibly playing while you dictate, and play it
//! again only when Hex saw it go silent right after its own pause.
//!
//! macOS no longer tells other apps what is playing, so Hex listens to the
//! browsers' own output: the level says whether the system pause reached a
//! browser, and only that proof allows the play at the end.

use std::time::Duration;

/// Sound: a peak above this over the first listen.
const SOUND_DBFS: f32 = -60.0;
/// Silence: a peak below this in one window after the pause.
const SILENCE_DBFS: f32 = -90.0;
const LISTEN: Duration = Duration::from_millis(300);
const CONFIRM_WINDOW: Duration = Duration::from_millis(100);
const CONFIRM_WINDOWS: usize = 10;

/// The explicit MediaRemote commands. Never the toggle: it can start music.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub(crate) enum MediaCommand {
    Play = 0,
    Pause = 1,
}

pub(crate) trait BrowserAudio {
    /// A browser is using the microphone: probably a call.
    fn in_call(&mut self) -> bool;
    /// Peak of the browsers' output over `window`, in dBFS; None when it cannot listen.
    fn peak_dbfs(&mut self, window: Duration) -> Option<f32>;
    fn send(&mut self, command: MediaCommand) -> bool;
}

/// Proof that Hex paused a sounding browser; only this allows play.
#[derive(Debug)]
pub(crate) struct PausedBrowser(());

#[cfg(test)]
impl PausedBrowser {
    pub(crate) fn for_test() -> Self {
        Self(())
    }
}

pub(crate) fn pause(audio: &mut impl BrowserAudio) -> Option<PausedBrowser> {
    if audio.in_call() {
        tracing::info!("left the browser alone: it is using the microphone");
        return None;
    }
    match audio.peak_dbfs(LISTEN) {
        Some(peak) if peak > SOUND_DBFS => {}
        Some(_) => {
            tracing::info!("left the browser alone: it is silent");
            return None;
        }
        None => {
            tracing::info!("left the browser alone: cannot hear it");
            return None;
        }
    }
    if !audio.send(MediaCommand::Pause) {
        tracing::info!("the system refused the pause command");
        return None;
    }
    for _ in 0..CONFIRM_WINDOWS {
        if audio
            .peak_dbfs(CONFIRM_WINDOW)
            .is_some_and(|peak| peak < SILENCE_DBFS)
        {
            tracing::info!("paused the browser");
            return Some(PausedBrowser(()));
        }
    }
    tracing::info!("sent pause but the browser still plays; it will not get play");
    None
}

pub(crate) fn resume(audio: &mut impl BrowserAudio, _paused: PausedBrowser) {
    if audio
        .peak_dbfs(LISTEN)
        .is_some_and(|peak| peak > SOUND_DBFS)
    {
        tracing::info!("the browser already plays again; sent no play");
        return;
    }
    if audio.send(MediaCommand::Play) {
        tracing::info!("resumed the browser");
    } else {
        tracing::info!("the system refused the play command");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::time::Duration;

    use super::*;

    struct Fake {
        in_call: bool,
        peaks: VecDeque<Option<f32>>,
        sent: Vec<MediaCommand>,
        send_ok: bool,
        listened: usize,
    }

    impl Fake {
        fn new(peaks: impl IntoIterator<Item = Option<f32>>) -> Self {
            Self {
                in_call: false,
                peaks: peaks.into_iter().collect(),
                sent: Vec::new(),
                send_ok: true,
                listened: 0,
            }
        }
    }

    impl BrowserAudio for Fake {
        fn in_call(&mut self) -> bool {
            self.in_call
        }

        fn peak_dbfs(&mut self, _window: Duration) -> Option<f32> {
            self.listened += 1;
            self.peaks.pop_front().flatten()
        }

        fn send(&mut self, command: MediaCommand) -> bool {
            self.sent.push(command);
            self.send_ok
        }
    }

    #[test]
    fn a_silent_browser_is_left_alone() {
        let mut audio = Fake::new([Some(-120.0)]);
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, []);
    }

    #[test]
    fn a_call_is_never_paused() {
        let mut audio = Fake::new([Some(-20.0)]);
        audio.in_call = true;
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, []);
        assert_eq!(audio.listened, 0);
    }

    #[test]
    fn a_confirmed_pause_is_resumed() {
        let mut audio = Fake::new([Some(-20.0), Some(-30.0), Some(-200.0), Some(-200.0)]);
        let paused = pause(&mut audio).expect("the pause is confirmed");
        resume(&mut audio, paused);
        assert_eq!(audio.sent, [MediaCommand::Pause, MediaCommand::Play]);
    }

    #[test]
    fn two_sounding_tabs_never_get_play() {
        let mut audio =
            Fake::new(std::iter::once(Some(-20.0)).chain(std::iter::repeat_n(Some(-25.0), 10)));
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }

    #[test]
    fn no_level_sends_nothing() {
        let mut audio = Fake::new([None]);
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, []);
    }

    #[test]
    fn a_browser_already_playing_gets_no_play() {
        let mut audio = Fake::new([Some(-20.0), Some(-200.0)]);
        let paused = pause(&mut audio).expect("the pause is confirmed");
        audio.peaks = VecDeque::from([Some(-20.0)]);
        resume(&mut audio, paused);
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }

    #[test]
    fn a_refused_command_is_not_confirmed() {
        let mut audio = Fake::new([Some(-20.0), Some(-200.0)]);
        audio.send_ok = false;
        assert!(pause(&mut audio).is_none());
        assert_eq!(audio.sent, [MediaCommand::Pause]);
    }
}
