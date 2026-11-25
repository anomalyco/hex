//! A preview owns a copy of the clip and tuning, never the virtual microphone.
//! One worker owns the output sink; only the latest pending play survives.
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use super::{
    DemoPlaybackTrack, DemoSourceKind, HEX_MICROPHONE_SAMPLE_RATE, TunedFilterConfig,
    WAVEFORM_BARS, compute_peaks, span_for_source,
};

struct Clip {
    track: DemoPlaybackTrack,
    source: DemoSourceKind,
    samples: Vec<f32>,
    tuning: TunedFilterConfig,
}

impl Clip {
    fn render(mut self) -> Vec<f32> {
        let DemoPlaybackTrack::Effect(mode) = self.track else {
            return self.samples;
        };
        self.tuning.mode = mode;
        crate::audio::voice_filter::render(
            &self.samples,
            f64::from(HEX_MICROPHONE_SAMPLE_RATE),
            &self.tuning.build_mode_spec(),
            &[span_for_source(self.source, self.samples.len())],
            f64::from(self.tuning.strength),
        )
    }
}

#[derive(Default)]
struct Playback {
    revision: u64,
    worker: bool,
    pending: Option<Clip>,
    track: Option<DemoPlaybackTrack>,
    started: Option<(Instant, Duration)>,
    peaks: Option<Vec<f32>>,
}

impl Playback {
    fn replace(&mut self, next: Option<Clip>) {
        self.revision += 1;
        self.track = next.as_ref().map(|c| c.track);
        self.pending = next;
        self.started = None;
        self.peaks = None;
    }
}

static PLAYER: std::sync::LazyLock<Mutex<Playback>> = std::sync::LazyLock::new(Mutex::default);
static CHANGED: Condvar = Condvar::new();

pub(super) fn busy() -> bool {
    PLAYER.lock().unwrap().track.is_some()
}

pub(super) fn snapshot() -> (Option<DemoPlaybackTrack>, f32, Option<Vec<f32>>) {
    let state = PLAYER.lock().unwrap();
    let progress = state.started.map_or(0.0, |(start, len)| {
        (start.elapsed().as_secs_f32() / len.as_secs_f32()).min(1.0)
    });
    (state.track, progress, state.peaks.clone())
}

pub fn stop_demo_playback() {
    PLAYER.lock().unwrap().replace(None);
    CHANGED.notify_one();
}

pub fn play_demo_track(track: DemoPlaybackTrack) {
    super::stop_demo_recording();
    let clip = {
        let Ok(buffers) = super::demo_buffers_lock().lock() else {
            return;
        };
        Clip {
            track,
            source: buffers.source,
            samples: buffers.active_samples().to_vec(),
            tuning: super::current_tuned_config(),
        }
    };
    let mut state = PLAYER.lock().unwrap();
    state.replace(Some(clip));
    if !state.worker {
        state.worker = true;
        std::thread::spawn(run);
    }
    CHANGED.notify_one();
}

fn run() {
    loop {
        let (revision, clip) = {
            let mut state = PLAYER.lock().unwrap();
            let Some(clip) = state.pending.take() else {
                state.worker = false;
                return;
            };
            (state.revision, clip)
        };
        play(revision, clip);
        let mut state = PLAYER.lock().unwrap();
        if state.revision == revision {
            state.replace(None);
        }
    }
}

fn play(revision: u64, clip: Clip) {
    let track = clip.track;
    let samples = clip.render();
    if samples.is_empty() || PLAYER.lock().unwrap().revision != revision {
        return;
    }
    let peaks = matches!(track, DemoPlaybackTrack::Effect(_))
        .then(|| compute_peaks(&samples, WAVEFORM_BARS));
    let duration =
        Duration::from_secs_f64(samples.len() as f64 / f64::from(HEX_MICROPHONE_SAMPLE_RATE));
    let Ok(mut sink) = crate::audio::open_default_output_sink() else {
        return;
    };
    sink.log_on_drop(false);
    let buffer = rodio::buffer::SamplesBuffer::new(
        1.try_into().unwrap(),
        HEX_MICROPHONE_SAMPLE_RATE.try_into().unwrap(),
        samples,
    );
    let mut state = PLAYER.lock().unwrap();
    if state.revision != revision {
        return;
    }
    sink.mixer().add(buffer);
    state.started = Some((Instant::now(), duration));
    state.peaks = peaks;
    drop(
        CHANGED
            .wait_timeout_while(state, duration, |s| s.revision == revision)
            .unwrap(),
    );
    // Drop the only sink before taking the next clip.
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::voice_filter::VoiceFilterMode::{Muffled, Muted, Robot};

    #[test]
    fn audition_changes_only_the_requested_span_and_only_on_its_copy() {
        let live = TunedFilterConfig::default();
        let input = vec![0.25; 5 * 48000];
        let render = |track| {
            Clip {
                track,
                source: DemoSourceKind::Recording,
                samples: input.clone(),
                tuning: live.clone(),
            }
            .render()
        };
        let muted = render(DemoPlaybackTrack::Effect(Muted));
        assert_eq!(muted.len(), input.len());
        assert!(muted[..48000] == input[..48000]);
        // The preset fades back to the original after the dictation span.
        assert!(muted[4 * 48000 + 24000..] == input[4 * 48000 + 24000..]);
        assert!(muted[2 * 48000].abs() < 0.01);
        assert_ne!(muted, render(DemoPlaybackTrack::Effect(Robot)));
        assert_eq!(render(DemoPlaybackTrack::You), input);
        assert_eq!(live.mode, Muffled);
    }

    #[test]
    fn rapid_auditions_replace_pending_audio_and_invalidate_work_in_flight() {
        let mut state = Playback::default();
        let clip = |track| Clip {
            track,
            source: DemoSourceKind::Example,
            samples: vec![],
            tuning: TunedFilterConfig::default(),
        };
        state.replace(Some(clip(DemoPlaybackTrack::Effect(Robot))));
        let rendering = state.revision;
        let _in_flight = state.pending.take().unwrap();
        state.replace(Some(clip(DemoPlaybackTrack::Effect(Muted))));
        state.replace(Some(clip(DemoPlaybackTrack::You)));
        assert_ne!(state.revision, rendering);
        assert_eq!(
            state.pending.as_ref().unwrap().track,
            DemoPlaybackTrack::You
        );
        state.replace(None);
        assert!(state.track.is_none() && state.pending.is_none());
    }
}
