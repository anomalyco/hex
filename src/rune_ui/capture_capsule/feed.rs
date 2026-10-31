//! The capsule's side of the capture: it follows the newest capture in
//! `capture_host`'s published snapshot from its start to its outcome, and
//! hands presses back to the listener as `hex capture` commands.

use std::path::PathBuf;
use std::time::Duration;

use scrawl_capture::capture::Stage;

use super::CaptureCapsule;
use super::flow::{Action, Delivered, Dest, Failure, Grant, Input, Mode, Route, Undone};
use super::ink::InkOverlay;
use super::picker::PickerOverlay;
use crate::capture_host::{
    CaptureCommand, CaptureNeed, CaptureRoute, CaptureSnapshot, CaptureTarget, NeedState,
    RouteAnswer, RouteDelivery, RouteDest,
};

/// The capture the capsule shows, the stage it last showed, and whether its
/// recording got to work.
#[derive(Debug, Default)]
pub(crate) struct Follow {
    shown: Option<(PathBuf, Stage)>,
    worked: bool,
    /// The engine started recording the shown capture, so a failure is one
    /// to save it, not to start it.
    began: bool,
    /// The settled capture the previous snapshot held: `None` before the
    /// first snapshot, so an outcome from before Hex started moves nothing.
    last_seen: Option<Option<PathBuf>>,
    /// The captures the previous snapshot held recording or finishing: a
    /// failure among them is a background capture's, never a start's.
    under_way: Vec<PathBuf>,
    /// The shown capture's route as last seen, and whether its answer
    /// reached the capsule.
    route: Option<CaptureRoute>,
    phone: bool,
}

impl Follow {
    /// What moves the capsule between the last snapshot and `snapshot`. A
    /// new recording takes the capsule over, with its switch chips when the
    /// shortcut started it on the pointer's display; its first click,
    /// scroll, or stroke folds them. A start that failed before any
    /// snapshot showed it recording still shows its failure. A capture it
    /// never showed otherwise (one that settled before Hex started, or one a
    /// chip discarded) moves nothing.
    pub(crate) fn next(&mut self, snapshot: &CaptureSnapshot) -> Vec<Input> {
        let mut inputs = Vec::new();
        if snapshot.phone.is_some() != self.phone {
            self.phone = snapshot.phone.is_some();
            inputs.push(Input::Phone(self.phone));
        }
        let last = snapshot.last.as_ref().map(|last| last.bundle.clone());
        let previous = self.last_seen.replace(last.clone());
        let under_way = std::mem::replace(
            &mut self.under_way,
            snapshot
                .recording
                .iter()
                .map(|recording| recording.bundle.clone())
                .chain(
                    snapshot
                        .finishing
                        .iter()
                        .map(|finishing| finishing.bundle.clone()),
                )
                .collect(),
        );
        // A start's failure, never one under a recording (that's newer) or
        // a capture's that was already under way.
        if let (Some(previous), Some(failed)) = (previous, &snapshot.last)
            && snapshot.recording.is_none()
            && !under_way.contains(&failed.bundle)
            && previous.as_ref() != Some(&failed.bundle)
            && failed.stage == Stage::Failed
            && self
                .shown
                .as_ref()
                .is_none_or(|(bundle, _)| *bundle != failed.bundle)
        {
            // Took the capsule over as it started: show it as followed.
            self.shown = Some((failed.bundle.clone(), Stage::Recording));
            self.began = false;
        }
        if let Some(recording) = &snapshot.recording {
            let fresh = self
                .shown
                .as_ref()
                .is_none_or(|(bundle, _)| *bundle != recording.bundle);
            if fresh {
                self.shown = Some((recording.bundle.clone(), Stage::Recording));
                self.route = None;
                self.worked = recording.worked;
                self.began = recording.screen;
                // No target chips: Annotate records the screen, and the
                // capsule's toolbar (the pen) takes their place.
                inputs.push(Input::Stage(Stage::Recording));
            } else if recording.worked && !self.worked {
                self.worked = true;
                inputs.push(Input::Worked);
            }
            self.began |= recording.screen;
            return inputs;
        }
        let stage = self.stage(snapshot);
        if stage == Some(Input::Stage(Stage::Done)) && heard_nothing(snapshot, self.shown.as_ref())
        {
            inputs.push(Input::NothingHeard);
        }
        inputs.extend(stage);
        inputs.extend(self.routed(snapshot));
        inputs
    }

    /// What the router's news for the shown capture tells the capsule. The
    /// capsule shows a rule's or a chip's send as sent itself, so only what
    /// it can't know moves it: the answer, the send going (for its sound),
    /// a send OpenCode didn't take (and its later retry), how an Undo
    /// ended, and a failed send.
    fn routed(&mut self, snapshot: &CaptureSnapshot) -> Vec<Input> {
        let Some((bundle, _)) = &self.shown else {
            return Vec::new();
        };
        let Some(route) = snapshot
            .last
            .as_ref()
            .filter(|last| last.bundle == *bundle)
            .and_then(|last| last.route.clone())
        else {
            return Vec::new();
        };
        let before = self.route.replace(route.clone());
        if before.as_ref() == Some(&route) {
            return Vec::new();
        }
        let mut inputs = Vec::new();
        let answered = |route: Option<&CaptureRoute>| {
            route.is_some_and(|route| route.answer != RouteAnswer::Looking)
        };
        if answered(Some(&route)) && !answered(before.as_ref()) {
            inputs.extend(capsule_route(&route.answer).map(Input::Routed));
        }
        let was = before.and_then(|before| before.delivery);
        if route.delivery == was {
            return inputs;
        }
        match route.delivery {
            Some(RouteDelivery::Queued { to, why }) => {
                inputs.push(Input::Delivered(Delivered::Queued { to, why }));
            }
            Some(RouteDelivery::Sent { to })
                if matches!(was, Some(RouteDelivery::Queued { .. })) =>
            {
                inputs.push(Input::Delivered(Delivered::Sent { to }));
            }
            // A send the capsule knows of, or one from `hex capture send`.
            Some(RouteDelivery::Sent { .. }) => inputs.push(Input::Settled),
            Some(RouteDelivery::TakenBack) => inputs.push(Input::Undone(Undone::TakenBack)),
            Some(RouteDelivery::TooLate { .. }) => inputs.push(Input::Undone(Undone::TooLate)),
            Some(RouteDelivery::Failed { to, why }) => {
                inputs.push(Input::Failed(Failure::Send(to), Some(why)));
            }
            _ => {}
        }
        inputs
    }

    fn stage(&mut self, snapshot: &CaptureSnapshot) -> Option<Input> {
        let followed = self.shown.as_ref().map(|(bundle, _)| bundle);
        let (bundle, stage) = if let Some(finishing) = snapshot
            .finishing
            .iter()
            .find(|finishing| Some(&finishing.bundle) == followed)
        {
            let stage = if finishing.cancelling {
                Stage::Cancelled
            } else {
                finishing.stage
            };
            (&finishing.bundle, stage)
        } else if let Some(last) = snapshot
            .last
            .as_ref()
            .filter(|last| Some(&last.bundle) == followed)
        {
            (&last.bundle, last.stage)
        } else {
            // A recording shown that left with no outcome: discarded. It
            // never comes back.
            self.shown
                .take_if(|(_, stage)| *stage == Stage::Recording)?;
            return Some(Input::Gone);
        };
        if self
            .shown
            .as_ref()
            .is_some_and(|shown| shown.0 == *bundle && shown.1 == stage)
        {
            return None;
        }
        self.shown = Some((bundle.clone(), stage));
        if stage == Stage::Failed {
            let error = snapshot
                .last
                .as_ref()
                .filter(|last| last.bundle == *bundle)
                .and_then(|last| last.error.clone());
            let what = if self.began {
                Failure::Save
            } else {
                Failure::Start
            };
            return Some(Input::Failed(what, error));
        }
        Some(Input::Stage(stage))
    }
}

/// Whether the shown capture was written with a transcript that has no
/// words. One that has no transcript at all (the transcriber was missing
/// or failed) wasn't silence.
fn heard_nothing(snapshot: &CaptureSnapshot, shown: Option<&(PathBuf, Stage)>) -> bool {
    snapshot.last.as_ref().is_some_and(|last| {
        shown.is_some_and(|(bundle, _)| *bundle == last.bundle)
            && last.words == Some(0)
            && last.missing_transcript.is_none()
    })
}

/// The router's answer as the capsule shows it. A rule's own session leads
/// its suggestions, as in Scrawl's list; the capsule shows sessions by
/// title and sends to them by id.
fn capsule_route(answer: &RouteAnswer) -> Option<Route> {
    let dests = |dests: &[RouteDest]| -> Vec<Dest> {
        dests
            .iter()
            .map(|dest| {
                Dest::new(dest.name.clone(), dest.why.clone()).session(dest.session.clone())
            })
            .collect()
    };
    Some(match answer {
        RouteAnswer::Looking => return None,
        RouteAnswer::Rule {
            to,
            suggested,
            recent,
            site,
        } => Route::Rule {
            to: dests(std::slice::from_ref(to)).remove(0),
            suggested: dests(std::slice::from_ref(to))
                .into_iter()
                .chain(dests(suggested))
                .collect(),
            recent: dests(recent),
            site: site.clone(),
        },
        RouteAnswer::Guess {
            suggested,
            recent,
            site,
        } => Route::Guess {
            suggested: dests(suggested),
            recent: dests(recent),
            site: site.clone(),
        },
        RouteAnswer::Nothing { recent } => Route::Nothing {
            recent: dests(recent),
        },
        RouteAnswer::NoAnswer { recent } => Route::NoAnswer {
            recent: dests(recent),
        },
        RouteAnswer::Unreachable { why } => Route::Unreachable { why: why.clone() },
    })
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
/// The capsule, its picker, and the ink, installed together.
pub(crate) struct CaptureOverlays {
    capsule: CaptureCapsule,
    picker: PickerOverlay,
    ink: InkOverlay,
    /// Whether screen recordings may see them.
    shown: bool,
}

impl CaptureOverlays {
    /// Adds the three overlays, prewarmed and hidden, and starts following
    /// captures.
    pub(crate) fn install<S: 'static>(app: &mut rune::App<S>) -> Self {
        let picker = PickerOverlay::install(app, act);
        let aim = picker.clone();
        let capsule = CaptureCapsule::install(app, move |action| act(aim.resolve(action)));
        capsule.attach(picker.clone());
        let ink = InkOverlay::install(app);
        let lit = capsule.clone();
        super::ink::on_drawing(move |on| lit.input(Input::Drawing(on)));
        follow(capsule.clone(), ink.clone());
        let helper = capsule.clone();
        crate::capture_host::on_missing(move |need| show_helper(&helper, need));
        Self {
            capsule,
            picker,
            ink,
            shown: crate::capture_host::own_windows_recorded(),
        }
    }

    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    /// Follows `capture.show_overlays_in_recordings` on windows already
    /// made; returns the new value when it changed.
    pub(crate) fn follow_recordings(&mut self) -> Option<bool> {
        let shown = crate::capture_host::own_windows_recorded();
        if shown == self.shown {
            return None;
        }
        self.shown = shown;
        self.capsule.show_in_recordings(shown);
        self.picker.show_in_recordings(shown);
        self.ink.show_in_recordings(shown);
        Some(shown)
    }
}

/// Moves `capsule` with every published capture change, and wakes `ink` to
/// look for a recording capture's strokes, on its own thread for the life
/// of the process.
pub(crate) fn follow(capsule: CaptureCapsule, ink: InkOverlay) {
    let spawned = std::thread::Builder::new()
        .name("capture-capsule".into())
        .spawn(move || {
            let watch = crate::capture_host::watch();
            let mut follow = Follow::default();
            let images = rune::ImageCache::new(std::sync::Arc::new(|| {}));
            loop {
                let Some(snapshot) = watch.wait(Duration::from_secs(3600)) else {
                    continue;
                };
                ink.wake();
                let inputs = follow.next(&snapshot);
                // The first frame goes ahead of "Copied", so the capsule
                // lands with it rather than swapping its check a moment on.
                let mut screenless = false;
                if inputs.contains(&Input::Stage(Stage::Done))
                    && let Some((bundle, _)) = &follow.shown
                {
                    screenless = !bundle.join("frames/frame-01.png").exists();
                    if let Some(image) = first_frame(&images, bundle) {
                        capsule.thumbnail(image);
                    }
                }
                for input in inputs {
                    // The common first-capture failure, whose own reason is
                    // a ScreenCaptureKit error: the helper says what to do.
                    if let Input::Failed(Failure::Start | Failure::Save, _) = &input
                        && screen_off()
                    {
                        show_helper(&capsule, CaptureNeed::ScreenRecording);
                        continue;
                    }
                    capsule.input(input);
                }
                // Sent with only the voice and marks: the helper follows
                // once its capsule folds.
                if screenless && screen_off() {
                    show_helper(&capsule, CaptureNeed::ScreenRecording);
                }
            }
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "could not start the capture capsule's thread");
    }
}

/// Follows `need` while the permission helper shows it, once a second on
/// its own thread, until it's granted or the helper would have folded. One
/// watch at a time: a later refusal hands the running one its need.
fn watch_grant(capsule: CaptureCapsule, need: CaptureNeed) {
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    static WATCHING: AtomicBool = AtomicBool::new(false);
    static NEED: Mutex<Option<CaptureNeed>> = Mutex::new(None);
    *NEED.lock().unwrap_or_else(|e| e.into_inner()) = Some(need);
    if WATCHING.swap(true, Ordering::AcqRel) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("capture-permission".into())
        .spawn(move || {
            let started = std::time::Instant::now();
            while started.elapsed().as_secs_f64() < super::flow::PERMISSION_HOLD_S {
                let Some(need) = *NEED.lock().unwrap_or_else(|e| e.into_inner()) else {
                    break;
                };
                let grant = match crate::capture_host::permission_state(need, false) {
                    None => Grant::Ready,
                    Some(NeedState::Restart) => Grant::Restart,
                    Some(NeedState::Off) => Grant::Off,
                };
                capsule.input(Input::Permission(need, grant));
                if grant == Grant::Ready {
                    break;
                }
                std::thread::sleep(Duration::from_secs(1));
            }
            WATCHING.store(false, Ordering::Release);
        });
    if let Err(error) = spawned {
        WATCHING.store(false, Ordering::Release);
        tracing::warn!(%error, "could not watch a capture permission");
    }
}

/// A written capture's first keyframe (or its contact sheet), decoded on
/// the image cache's worker while this thread waits; `None` when neither
/// reads or decodes.
fn first_frame(images: &rune::ImageCache, bundle: &std::path::Path) -> Option<rune::Image> {
    ["frames/frame-01.png", "contact-sheet.png"]
        .into_iter()
        .find_map(|name| {
            let bytes = std::fs::read(bundle.join(name)).ok()?;
            let image = images.load(bytes);
            image.wait().ok()?;
            Some(image)
        })
}

/// Carries out a press or key on the capsule: Finish and Cancel end the
/// capture as `hex capture stop|cancel` would, a chip discards the
/// recording, the picker starts what it picked, and Record (or the
/// shortcut) without a pick records the display under the pointer. Runs on the
/// main thread, so it only queues the commands.
pub(crate) fn act(action: Action) {
    use crate::capture_host::CaptureControl;
    use crate::control::ScrawlCaptureAction;
    let scrawl = CaptureControl::Scrawl;
    let command = CaptureControl::Command;
    let screen = || CaptureCommand::Start(CaptureTarget::Display { at: pointer() });
    let controls: Vec<CaptureControl> = match action {
        Action::Finish => vec![scrawl(ScrawlCaptureAction::Stop)],
        Action::Cancel => vec![scrawl(ScrawlCaptureAction::Cancel)],
        // The picker opens: look for a phone cabled since the recording
        // began, as Scrawl's reveal does.
        Action::Discard => vec![
            command(CaptureCommand::Discard),
            command(CaptureCommand::FindPhone),
        ],
        Action::Start(target) => vec![command(CaptureCommand::Start(target))],
        // What the picker would record, when it has nothing to record yet
        // (its windows still load): the pointer's display.
        // Wayland lists no windows: the portal's own picker chooses one.
        Action::Record(Mode::Window) if cfg!(target_os = "linux") => {
            vec![command(CaptureCommand::Start(CaptureTarget::Window {
                id: 0,
            }))]
        }
        Action::Record(Mode::Screen | Mode::Window | Mode::Area) => vec![command(screen())],
        Action::Record(Mode::Iphone) => vec![command(CaptureCommand::Start(CaptureTarget::Phone))],
        Action::Switch(Mode::Iphone) => {
            vec![command(CaptureCommand::Switch(CaptureTarget::Phone))]
        }
        // Screen is what the shortcut records; Window and Area discard.
        Action::Switch(Mode::Screen | Mode::Window | Mode::Area) => return,
        Action::Send { to, always } => vec![command(CaptureCommand::Send {
            to,
            always: always.is_some(),
        })],
        Action::Undo { .. } => vec![command(CaptureCommand::Undo)],
        Action::Pen => {
            crate::capture_host::toggle_pen();
            return;
        }
        Action::Paste => vec![command(CaptureCommand::Paste)],
        Action::Inspect => vec![command(CaptureCommand::Inspect)],
        Action::Allow(need) => {
            crate::capture_host::allow_permission(need);
            return;
        }
        Action::Restart => {
            crate::capture_host::restart_for_permission();
            return;
        }
    };
    for control in controls {
        if let Err(error) = crate::capture_host::send_to_listener(control) {
            tracing::warn!(%error, "the capture capsule could not reach the listener");
        }
    }
}

/// Whether this Hex can't record the screen now, read afresh.
fn screen_off() -> bool {
    crate::capture_host::permission_state(CaptureNeed::ScreenRecording, true).is_some()
}

/// Opens the permission helper for `need` and follows its grant.
fn show_helper(capsule: &CaptureCapsule, need: CaptureNeed) {
    capsule.input(Input::NeedsPermission(need));
    watch_grant(capsule.clone(), need);
}

/// The pointer. Wayland tells Hex nothing of it: the portal picks the
/// screen.
#[cfg(target_os = "linux")]
fn pointer() -> [f64; 2] {
    [0.0, 0.0]
}

/// The pointer, in global physical pixels, as the engine's targets are.
#[cfg(target_os = "windows")]
fn pointer() -> [f64; 2] {
    scrawl_capture::screens::pointer().unwrap_or_default()
}

/// The pointer, in global top-left points.
#[cfg(target_os = "macos")]
fn pointer() -> [f64; 2] {
    use core_graphics_types::geometry::CGPoint;
    use std::ffi::c_void;
    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGEventCreate(source: *mut c_void) -> *mut c_void;
        fn CGEventGetLocation(event: *mut c_void) -> CGPoint;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFRelease(cf: *const c_void);
    }
    // SAFETY: a null source makes an event at the current pointer; it is
    // released once read.
    unsafe {
        let event = CGEventCreate(std::ptr::null_mut());
        if event.is_null() {
            return [0.0, 0.0];
        }
        let at = CGEventGetLocation(event);
        CFRelease(event);
        [at.x, at.y]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::CaptureInstant;
    use crate::capture_host::{FinishingCapture, RecordingCapture, SettledCapture};

    fn bundle(name: &str) -> PathBuf {
        PathBuf::from(format!("/captures/{name}.scrawl"))
    }

    fn recording(name: &str) -> CaptureSnapshot {
        CaptureSnapshot {
            generation: 0,
            recording: Some(RecordingCapture {
                bundle: bundle(name),
                since: CaptureInstant::now(),
                screen: true,
                target: crate::capture_host::TargetKind::Display,
                worked: false,
            }),
            finishing: Vec::new(),
            last: None,
            phone: None,
        }
    }

    fn finishing(name: &str, stage: Stage, cancelling: bool) -> CaptureSnapshot {
        CaptureSnapshot {
            generation: 0,
            recording: None,
            finishing: vec![FinishingCapture {
                bundle: bundle(name),
                stage,
                cancelling,
            }],
            last: None,
            phone: None,
        }
    }

    fn settled(name: &str, stage: Stage) -> CaptureSnapshot {
        CaptureSnapshot {
            generation: 0,
            recording: None,
            finishing: Vec::new(),
            last: Some(SettledCapture {
                bundle: bundle(name),
                stage,
                words: Some(12),
                gaps: Some(0),
                missing_transcript: None,
                error: None,
                route: None,
            }),
            phone: None,
        }
    }

    #[test]
    fn it_follows_a_capture_from_start_to_outcome() {
        let mut follow = Follow::default();
        assert_eq!(follow.next(&settled("old", Stage::Done)), []);
        let a = recording("a");
        assert_eq!(follow.next(&a), [Input::Stage(Stage::Recording)]);
        assert_eq!(follow.next(&a), [], "a repeat moves nothing");
        let stopping = finishing("a", Stage::Stopping, false);
        assert_eq!(follow.next(&stopping), [Input::Stage(Stage::Stopping)]);
        assert_eq!(
            follow.next(&finishing("a", Stage::Transcribing, false)),
            [Input::Stage(Stage::Transcribing)]
        );
        assert_eq!(
            follow.next(&settled("a", Stage::Done)),
            [Input::Stage(Stage::Done)]
        );
        assert_eq!(follow.next(&settled("a", Stage::Done)), []);
    }

    #[test]
    fn a_capture_with_no_words_says_nothing_was_heard() {
        let silent = |missing: Option<&str>| {
            let mut snapshot = settled("a", Stage::Done);
            let last = snapshot.last.as_mut().unwrap();
            last.words = Some(0);
            last.missing_transcript = missing.map(Into::into);
            snapshot
        };
        let mut follow = Follow::default();
        follow.next(&recording("a"));
        assert_eq!(
            follow.next(&silent(None)),
            [Input::NothingHeard, Input::Stage(Stage::Done)]
        );
        let mut follow = Follow::default();
        follow.next(&recording("a"));
        assert_eq!(
            follow.next(&silent(Some("Transcriber unavailable."))),
            [Input::Stage(Stage::Done)],
            "no transcript at all isn't silence"
        );
    }

    #[test]
    fn a_new_recording_takes_over_and_a_cancel_hides() {
        let mut follow = Follow::default();
        follow.next(&recording("a"));
        follow.next(&finishing("a", Stage::Writing, false));
        // A second capture starts while the first is written.
        let mut both = recording("b");
        both.finishing = finishing("a", Stage::Writing, false).finishing;
        assert_eq!(follow.next(&both), [Input::Stage(Stage::Recording)]);
        // The first settling no longer moves the capsule.
        let mut a_done = recording("b");
        a_done.last = settled("a", Stage::Done).last;
        assert_eq!(follow.next(&a_done), []);
        assert_eq!(
            follow.next(&finishing("b", Stage::Stopping, true)),
            [Input::Stage(Stage::Cancelled)]
        );
    }

    #[test]
    fn a_start_that_failed_unseen_still_shows_why() {
        let mut follow = Follow::default();
        assert_eq!(follow.next(&settled("old", Stage::Failed)), []);
        let mut failed = settled("a", Stage::Failed);
        failed.last.as_mut().unwrap().error = Some("no screen to record".into());
        assert_eq!(
            follow.next(&failed),
            [Input::Failed(
                Failure::Start,
                Some("no screen to record".into())
            )]
        );
        assert_eq!(follow.next(&failed), []);
    }

    #[test]
    fn a_capture_that_never_began_failed_to_start() {
        let mut follow = Follow::default();
        let mut starting = recording("a");
        starting.recording.as_mut().unwrap().screen = false;
        follow.next(&starting);
        let mut failed = settled("a", Stage::Failed);
        failed.last.as_mut().unwrap().error = Some("Screen Recording refused".into());
        assert_eq!(
            follow.next(&failed),
            [Input::Failed(
                Failure::Start,
                Some("Screen Recording refused".into())
            )]
        );
    }

    #[test]
    fn a_failure_says_why() {
        let mut follow = Follow::default();
        follow.next(&recording("a"));
        let mut failed = settled("a", Stage::Failed);
        failed.last.as_mut().unwrap().error = Some("the window closed".into());
        assert_eq!(
            follow.next(&failed),
            [Input::Failed(
                Failure::Save,
                Some("the window closed".into())
            )]
        );
    }

    #[test]
    fn the_shortcuts_recording_shows_no_target_chips() {
        let mut follow = Follow::default();
        let mut a = recording("a");
        a.recording.as_mut().unwrap().target = crate::capture_host::TargetKind::Pointer;
        assert_eq!(follow.next(&a), [Input::Stage(Stage::Recording)]);
        a.recording.as_mut().unwrap().worked = true;
        assert_eq!(follow.next(&a), [Input::Worked]);
        assert_eq!(follow.next(&a), []);
        assert_eq!(follow.next(&CaptureSnapshot::default()), [Input::Gone]);
        assert_eq!(follow.next(&CaptureSnapshot::default()), []);
    }

    /// A recording discarded with no outcome (and, from an older host, a
    /// switch whose start failed): the timer must not count on.
    #[test]
    fn a_recording_that_leaves_with_no_outcome_ends_the_capsule() {
        let mut follow = Follow::default();
        follow.next(&recording("a"));
        assert_eq!(follow.next(&CaptureSnapshot::default()), [Input::Gone]);
        // The host's failure for the start that never showed follows.
        let mut failed = settled("b", Stage::Failed);
        failed.last.as_mut().unwrap().error = Some("too many captures finishing".into());
        assert_eq!(
            follow.next(&failed),
            [Input::Failed(
                Failure::Start,
                Some("too many captures finishing".into())
            )]
        );
    }

    #[test]
    fn a_background_failure_never_takes_over_the_capture_it_follows() {
        let mut follow = Follow::default();
        follow.next(&recording("a"));
        let mut both = recording("b");
        both.finishing = finishing("a", Stage::Writing, false).finishing;
        assert_eq!(follow.next(&both), [Input::Stage(Stage::Recording)]);
        // "a" fails while "b" records: the timer and chips stay as they are.
        let mut a_failed = recording("b");
        a_failed.last = settled("a", Stage::Failed).last;
        assert_eq!(follow.next(&a_failed), []);
        // "c", written beside "b", fails: "b" stays shown.
        let mut b_writing = finishing("b", Stage::Writing, false);
        b_writing
            .finishing
            .extend(finishing("c", Stage::Writing, false).finishing);
        assert_eq!(follow.next(&b_writing), [Input::Stage(Stage::Writing)]);
        let mut c_failed = finishing("b", Stage::Writing, false);
        c_failed.last = settled("c", Stage::Failed).last;
        assert_eq!(follow.next(&c_failed), []);
        assert_eq!(
            follow.next(&settled("b", Stage::Done)),
            [Input::Stage(Stage::Done)]
        );
    }

    /// A start that fails before any snapshot shows it is the newest thing
    /// asked for, so it shows even while an older capture is written.
    #[test]
    fn a_start_that_fails_unseen_shows_over_an_older_capture() {
        let mut follow = Follow::default();
        follow.next(&recording("a"));
        follow.next(&finishing("a", Stage::Writing, false));
        let mut b_failed = finishing("a", Stage::Writing, false);
        b_failed.last = settled("b", Stage::Failed).last;
        assert_eq!(
            follow.next(&b_failed),
            [Input::Failed(Failure::Start, None)]
        );
    }

    fn dest(name: &str, why: &str) -> RouteDest {
        RouteDest {
            session: format!("ses_{name}"),
            name: name.into(),
            why: why.into(),
        }
    }

    fn routed(name: &str, answer: RouteAnswer, delivery: Option<RouteDelivery>) -> CaptureSnapshot {
        let mut snapshot = settled(name, Stage::Done);
        snapshot.last.as_mut().unwrap().route = Some(CaptureRoute {
            answer,
            delivery,
            offer: None,
        });
        snapshot
    }

    #[test]
    fn a_rule_shows_once_and_only_news_the_capsule_cant_know_moves_it() {
        let mut follow = Follow::default();
        follow.next(&recording("a"));
        assert_eq!(
            follow.next(&routed("a", RouteAnswer::Looking, None)),
            [Input::Stage(Stage::Done)]
        );
        let rule = || RouteAnswer::Rule {
            to: dest("SEER", "rule"),
            suggested: vec![dest("HEX", "last sent")],
            recent: vec![dest("HOME", "yesterday")],
            site: Some("seer.example.com".into()),
        };
        // The router sends a rule's answer at once.
        let sending = || Some(RouteDelivery::Sending { to: "SEER".into() });
        assert_eq!(
            follow.next(&routed("a", rule(), sending())),
            [Input::Routed(Route::Rule {
                to: Dest::new("SEER", "rule").session("ses_SEER"),
                suggested: vec![
                    Dest::new("SEER", "rule").session("ses_SEER"),
                    Dest::new("HEX", "last sent").session("ses_HEX"),
                ],
                recent: vec![Dest::new("HOME", "yesterday").session("ses_HOME")],
                site: Some("seer.example.com".into()),
            })]
        );
        assert_eq!(follow.next(&routed("a", rule(), sending())), []);
        // Sent settles it (Send to… no longer brings it back) and
        // sounds Sent.
        let sent = Some(RouteDelivery::Sent { to: "SEER".into() });
        assert_eq!(follow.next(&routed("a", rule(), sent)), [Input::Settled]);
        assert_eq!(
            follow.next(&routed("a", rule(), Some(RouteDelivery::TakenBack))),
            [Input::Undone(Undone::TakenBack)]
        );
    }

    #[test]
    fn a_queued_send_says_so_and_says_sent_when_it_goes() {
        let mut follow = Follow::default();
        follow.next(&recording("a"));
        let guess = || RouteAnswer::Guess {
            suggested: vec![dest("HEX", "same page")],
            recent: Vec::new(),
            site: None,
        };
        follow.next(&routed("a", guess(), None));
        let queued = Some(RouteDelivery::Queued {
            to: "HEX".into(),
            why: "OpenCode can't see files".into(),
        });
        assert_eq!(
            follow.next(&routed("a", guess(), queued)),
            [Input::Delivered(Delivered::Queued {
                to: "HEX".into(),
                why: "OpenCode can't see files".into(),
            })]
        );
        let sent = Some(RouteDelivery::Sent { to: "HEX".into() });
        assert_eq!(
            follow.next(&routed("a", guess(), sent)),
            [Input::Delivered(Delivered::Sent { to: "HEX".into() })]
        );
        let failed = Some(RouteDelivery::Failed {
            to: "HEX".into(),
            why: "gone".into(),
        });
        assert_eq!(
            follow.next(&routed("a", guess(), failed)),
            [Input::Failed(
                Failure::Send("HEX".into()),
                Some("gone".into())
            )]
        );
    }

    #[test]
    fn a_cabled_phone_shows_its_chip() {
        let mut follow = Follow::default();
        let mut snapshot = CaptureSnapshot {
            phone: Some("Kit's iPhone".into()),
            ..CaptureSnapshot::default()
        };
        assert_eq!(follow.next(&snapshot), [Input::Phone(true)]);
        assert_eq!(follow.next(&snapshot), []);
        snapshot.phone = None;
        assert_eq!(follow.next(&snapshot), [Input::Phone(false)]);
    }
}
