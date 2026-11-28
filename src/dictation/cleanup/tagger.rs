//! The warm tagger: one loaded model on one thread, shared by every
//! dictation.
//!
//! The model loads once, the first time a dictation wants it, and stays
//! loaded while Labs › Clean up what I say is on. It lives on a thread of
//! its own, so a dictation only ever waits for an answer until its deadline:
//! a load or a run that takes longer finishes behind it, the dictation
//! pastes as heard, and the model is ready for the next one. Turning the lab
//! off ends the thread, which frees the model, and Labs › Free memory when
//! idle frees it after the same quiet time as the speech model.
//!
//! On macOS the model is Core ML's, restricted to the CPU
//! (`cleanup_tagger.swift`). No other OS runs it yet. Labs › Clean up what
//! I say v2 runs its own model on the same kind of thread (`v2::runtime`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::chunk::Chunk;
use super::stage::Skip;

/// What the tagger's thread needs of a model: Core ML in the app, a stub in
/// tests.
pub(super) trait Runtime: Clone + Send + 'static {
    type Model;
    /// What one run reads: a chunk of token ids for v1.
    type Input: Send + 'static;
    /// What one run answers: a DELETE logit per token for v1.
    type Output: Send + 'static;

    /// Loads the model, or says why there is none to load.
    fn load(&self) -> Result<Self::Model, Skip>;

    /// One run over one input.
    fn run(model: &Self::Model, input: &Self::Input) -> Result<Self::Output, String>;

    /// How long a loaded model may sit unused before it is freed, when it
    /// may not stay loaded for good.
    fn idle(&self) -> Option<Duration>;
}

/// A dictation's inputs, waiting for their answers.
struct Request<R: Runtime> {
    inputs: Vec<R::Input>,
    deadline: Instant,
    reply: mpsc::Sender<Result<Vec<R::Output>, Skip>>,
}

/// The running thread: its inbox (a None job loads the model only) and
/// whether its model is loaded.
struct Thread<R: Runtime> {
    jobs: mpsc::Sender<Option<Request<R>>>,
    loaded: Arc<AtomicBool>,
}

/// The one model and the thread it lives on.
pub(super) struct Tagger<R: Runtime> {
    runtime: R,
    thread: Mutex<Option<Thread<R>>>,
}

impl<R: Runtime> Tagger<R> {
    pub(super) const fn new(runtime: R) -> Self {
        Self {
            runtime,
            thread: Mutex::new(None),
        }
    }

    /// Whether the model is loaded and warm, so an answer takes a run and
    /// not a load.
    pub(super) fn loaded(&self) -> bool {
        self.thread
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .is_some_and(|thread| thread.loaded.load(Ordering::Acquire))
    }

    /// The model's answer for each input, in order, or why there is none
    /// by `deadline`.
    pub(super) fn score(
        &self,
        inputs: Vec<R::Input>,
        deadline: Instant,
    ) -> Result<Vec<R::Output>, Skip> {
        let (reply, answer) = mpsc::channel();
        self.send(Some(Request {
            inputs,
            deadline,
            reply,
        }))?;
        match answer.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(scores) => scores,
            Err(RecvTimeoutError::Timeout) => Err(Skip::Late),
            // The thread drops a request it reaches after its deadline.
            Err(RecvTimeoutError::Disconnected) if Instant::now() >= deadline => Err(Skip::Late),
            Err(RecvTimeoutError::Disconnected) => {
                Err(Skip::Failed("the cleanup model stopped".into()))
            }
        }
    }

    /// Loads the model in the background, so the next dictation finds it
    /// loaded.
    pub(super) fn warm(&self) {
        let _ = self.send(None);
    }

    /// Ends the thread once it finishes what it is doing, which frees the
    /// model. The next request starts a new one.
    pub(super) fn release(&self) {
        self.thread
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take();
    }

    fn send(&self, job: Option<Request<R>>) -> Result<(), Skip> {
        let mut thread = self
            .thread
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let job = match thread.as_ref() {
            Some(running) => match running.jobs.send(job) {
                Ok(()) => return Ok(()),
                // The thread is gone: start another.
                Err(mpsc::SendError(job)) => job,
            },
            None => job,
        };
        let (jobs, inbox) = mpsc::channel();
        let loaded = Arc::new(AtomicBool::new(false));
        let runtime = self.runtime.clone();
        let marks = loaded.clone();
        std::thread::Builder::new()
            .name("hex-cleanup-tagger".into())
            .spawn(move || serve(&runtime, &inbox, &marks))
            .map_err(|error| Skip::Failed(format!("the cleanup model could not start: {error}")))?;
        let sent = jobs.send(job);
        *thread = Some(Thread { jobs, loaded });
        sent.map_err(|_| Skip::Failed("the cleanup model stopped".into()))
    }
}

/// The tagger's thread: loads the model for the first job that needs it and
/// answers each request in turn, until its tagger is released. A model left
/// idle too long is freed, and the next job loads it again.
fn serve<R: Runtime>(runtime: &R, inbox: &mpsc::Receiver<Option<Request<R>>>, loaded: &AtomicBool) {
    let mut model = None;
    loop {
        let job = match runtime.idle().filter(|_| model.is_some()) {
            Some(idle) => match inbox.recv_timeout(idle) {
                Ok(job) => job,
                Err(RecvTimeoutError::Timeout) => {
                    loaded.store(false, Ordering::Release);
                    model = None;
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match inbox.recv() {
                Ok(job) => job,
                Err(mpsc::RecvError) => return,
            },
        };
        if model.is_none() {
            match runtime.load() {
                Ok(ready) => {
                    model = Some(ready);
                    loaded.store(true, Ordering::Release);
                }
                Err(skip) => {
                    if let Some(request) = job {
                        let _ = request.reply.send(Err(skip));
                    }
                    continue;
                }
            }
        }
        let (Some(model), Some(request)) = (&model, job) else {
            continue;
        };
        // Its dictation has pasted as heard: nothing reads this answer.
        if Instant::now() >= request.deadline {
            continue;
        }
        let scores = request
            .inputs
            .iter()
            .map(|input| R::run(model, input))
            .collect::<Result<Vec<_>, _>>();
        let _ = request.reply.send(scores.map_err(Skip::Failed));
    }
}

#[cfg(all(target_os = "macos", not(test)))]
pub(super) use core_ml::CoreMl;
#[cfg(target_os = "macos")]
pub(super) use core_ml::{Loaded, compile, load, run, take_error};

/// Core ML on the CPU, behind the C bridge in `cleanup_tagger.swift`.
#[cfg(target_os = "macos")]
mod core_ml {
    use std::ffi::{CStr, CString, c_char, c_void};
    use std::path::Path;
    use std::ptr::{self, NonNull};

    unsafe extern "C" {
        fn hex_cleanup_tagger_compile(
            package: *const c_char,
            compiled: *const c_char,
            error: *mut *mut c_char,
        ) -> bool;
        fn hex_cleanup_tagger_load(compiled: *const c_char, error: *mut *mut c_char)
        -> *mut c_void;
        fn hex_cleanup_tagger_run(
            tagger: *mut c_void,
            input_ids: *const i32,
            padded_length: i32,
            length: i32,
            logits: *mut f32,
            error: *mut *mut c_char,
        ) -> bool;
        fn hex_cleanup_tagger_release(tagger: *mut c_void);
        fn hex_cleanup_tagger_free_string(string: *mut c_char);
    }

    /// The pinned model, loaded from where `model` installs it.
    #[cfg(not(test))]
    #[derive(Clone, Copy)]
    pub(in crate::dictation::cleanup) struct CoreMl;

    #[cfg(not(test))]
    impl super::Runtime for CoreMl {
        type Model = Loaded;
        type Input = super::Chunk;
        type Output = Vec<f32>;

        fn load(&self) -> Result<Loaded, super::Skip> {
            use super::super::model;
            use super::Skip;
            let folder = model::folder().map_err(|error| Skip::Failed(error.to_string()))?;
            let Some(compiled) = model::installed(&model::TAGGER, &folder) else {
                model::start_install();
                return Err(Skip::ModelMissing);
            };
            let started = std::time::Instant::now();
            let loaded = load(&compiled).map_err(|error| {
                tracing::warn!(%error, "could not load the cleanup model");
                // A compiled model Core ML cannot load is compiled again.
                let _ = std::fs::remove_dir_all(&compiled);
                model::start_install();
                Skip::Failed(error)
            })?;
            tracing::info!(
                elapsed_ms = started.elapsed().as_millis() as u64,
                "the cleanup model is loaded"
            );
            Ok(loaded)
        }

        fn run(model: &Loaded, chunk: &super::Chunk) -> Result<Vec<f32>, String> {
            run(model, &chunk.input_ids(), chunk.length())
        }

        /// Labs › Free memory when idle's time, while that lab is on.
        fn idle(&self) -> Option<std::time::Duration> {
            crate::labs::unload_idle()
        }
    }

    /// One loaded Core ML model, freed when dropped.
    pub(in crate::dictation::cleanup) struct Loaded(NonNull<c_void>);

    // SAFETY: The model is only ever run and freed by the thread that holds
    // it, one call at a time, and Core ML models are not tied to the thread
    // that loaded them.
    unsafe impl Send for Loaded {}

    impl Loaded {
        /// The bridge's handle, for a call that borrows the model.
        pub(in crate::dictation::cleanup) fn as_ptr(&self) -> *mut c_void {
            self.0.as_ptr()
        }
    }

    impl Drop for Loaded {
        fn drop(&mut self) {
            // SAFETY: The pointer came from `hex_cleanup_tagger_load` and is
            // given back exactly once.
            unsafe { hex_cleanup_tagger_release(self.0.as_ptr()) };
        }
    }

    /// Loads the compiled model at `compiled`, restricted to the CPU.
    pub(in crate::dictation::cleanup) fn load(compiled: &Path) -> Result<Loaded, String> {
        let path = c_path(compiled)?;
        let mut error = ptr::null_mut();
        // SAFETY: The path outlives the call and the bridge writes only
        // `error`.
        let loaded = unsafe { hex_cleanup_tagger_load(path.as_ptr(), &mut error) };
        NonNull::new(loaded)
            .map(Loaded)
            .ok_or_else(|| take_error(error))
    }

    /// One run over a chunk's padded ids: a DELETE logit for each of its
    /// `length` real tokens.
    pub(in crate::dictation::cleanup) fn run(
        model: &Loaded,
        input_ids: &[i32],
        length: usize,
    ) -> Result<Vec<f32>, String> {
        let (Ok(padded), Ok(real)) = (i32::try_from(input_ids.len()), i32::try_from(length)) else {
            return Err("the cleanup model was given too many words".into());
        };
        if length == 0 || length > input_ids.len() {
            return Err("the cleanup model was given no words".into());
        }
        let mut logits = vec![0.0_f32; length];
        let mut error = ptr::null_mut();
        // SAFETY: `input_ids` holds `padded` ids and `logits` holds `real`
        // values, both for the whole call, and the model stays loaded while
        // it is borrowed.
        let ran = unsafe {
            hex_cleanup_tagger_run(
                model.0.as_ptr(),
                input_ids.as_ptr(),
                padded,
                real,
                logits.as_mut_ptr(),
                &mut error,
            )
        };
        if ran {
            Ok(logits)
        } else {
            Err(take_error(error))
        }
    }

    /// Compiles the package at `package` into a compiled model at
    /// `compiled`, which must not exist.
    pub(in crate::dictation::cleanup) fn compile(
        package: &Path,
        compiled: &Path,
    ) -> Result<(), String> {
        let (package, compiled) = (c_path(package)?, c_path(compiled)?);
        let mut error = ptr::null_mut();
        // SAFETY: Both paths outlive the call and the bridge writes only
        // `error`.
        let done =
            unsafe { hex_cleanup_tagger_compile(package.as_ptr(), compiled.as_ptr(), &mut error) };
        if done { Ok(()) } else { Err(take_error(error)) }
    }

    fn c_path(path: &Path) -> Result<CString, String> {
        let path = path
            .to_str()
            .ok_or("the cleanup model's path is not valid UTF-8")?;
        CString::new(path).map_err(|error| error.to_string())
    }

    /// The bridge's error message, given back to it.
    pub(in crate::dictation::cleanup) fn take_error(error: *mut c_char) -> String {
        if error.is_null() {
            return "the cleanup model failed".into();
        }
        // SAFETY: The bridge returns NUL-terminated strings from `strdup`.
        let message = unsafe { CStr::from_ptr(error) }
            .to_string_lossy()
            .into_owned();
        // SAFETY: Ownership returns to the bridge allocator exactly once.
        unsafe { hex_cleanup_tagger_free_string(error) };
        message
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::*;
    use crate::dictation::cleanup::{Tokenizer, Words};

    /// A model that answers each token's id as its logit.
    #[derive(Clone, Default)]
    struct Stub {
        loads: Arc<AtomicUsize>,
        frees: Arc<AtomicUsize>,
        missing: bool,
        load_time: Duration,
        fails: bool,
        idle: Option<Duration>,
    }

    struct StubModel {
        frees: Arc<AtomicUsize>,
        fails: bool,
    }

    impl Drop for StubModel {
        fn drop(&mut self) {
            self.frees.fetch_add(1, Ordering::SeqCst);
        }
    }

    impl Runtime for Stub {
        type Model = StubModel;
        type Input = Chunk;
        type Output = Vec<f32>;

        fn load(&self) -> Result<StubModel, Skip> {
            if self.missing {
                return Err(Skip::ModelMissing);
            }
            std::thread::sleep(self.load_time);
            self.loads.fetch_add(1, Ordering::SeqCst);
            Ok(StubModel {
                frees: self.frees.clone(),
                fails: self.fails,
            })
        }

        fn run(model: &StubModel, chunk: &Chunk) -> Result<Vec<f32>, String> {
            if model.fails {
                return Err("Core ML could not run the model".into());
            }
            let ids = chunk.input_ids();
            Ok(ids[..chunk.length()].iter().map(|&id| id as f32).collect())
        }

        fn idle(&self) -> Option<Duration> {
            self.idle
        }
    }

    fn chunks(text: &str) -> Vec<Chunk> {
        super::super::stage::tokenizer()
            .map(|tokenizer: &Tokenizer| tokenizer.chunks(&Words::new(text)))
            .unwrap()
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    /// Waits for `count` to reach `wanted`, which another thread makes
    /// happen.
    fn eventually(count: &AtomicUsize, wanted: usize) {
        let give_up = Instant::now() + Duration::from_secs(30);
        while count.load(Ordering::SeqCst) != wanted {
            assert!(Instant::now() < give_up, "never reached {wanted}");
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn one_model_is_loaded_once_and_answers_every_chunk() {
        let stub = Stub::default();
        let tagger = Tagger::new(stub.clone());
        let first = tagger.score(chunks("um hello there"), soon()).unwrap();
        // [CLS] um hello there [SEP]
        assert_eq!(first, [[101.0, 8529.0, 7592.0, 2045.0, 102.0]]);
        let long = vec!["word"; 300].join(" ");
        let second = tagger.score(chunks(&long), soon()).unwrap();
        assert_eq!(
            second.iter().map(Vec::len).collect::<Vec<_>>(),
            [128, 128, 50]
        );
        assert_eq!(stub.loads.load(Ordering::SeqCst), 1);
        assert_eq!(stub.frees.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn turning_the_lab_off_frees_the_model_and_on_loads_it_again() {
        let stub = Stub::default();
        let tagger = Tagger::new(stub.clone());
        tagger.release();
        assert_eq!(stub.loads.load(Ordering::SeqCst), 0);
        tagger.warm();
        eventually(&stub.loads, 1);
        tagger.release();
        eventually(&stub.frees, 1);
        tagger.score(chunks("hello"), soon()).unwrap();
        assert_eq!(stub.loads.load(Ordering::SeqCst), 2);
        tagger.release();
        eventually(&stub.frees, 2);
    }

    #[test]
    fn a_tagger_says_whether_its_model_is_loaded() {
        let stub = Stub::default();
        let tagger = Tagger::new(stub.clone());
        assert!(!tagger.loaded());
        tagger.warm();
        let give_up = Instant::now() + Duration::from_secs(30);
        while !tagger.loaded() {
            assert!(Instant::now() < give_up, "never loaded");
            std::thread::sleep(Duration::from_millis(1));
        }
        tagger.release();
        assert!(!tagger.loaded());
    }

    #[test]
    fn a_model_left_idle_is_freed_and_loads_again_when_wanted() {
        let stub = Stub {
            idle: Some(Duration::from_millis(20)),
            ..Default::default()
        };
        let tagger = Tagger::new(stub.clone());
        tagger.score(chunks("um hello"), soon()).unwrap();
        assert_eq!(stub.loads.load(Ordering::SeqCst), 1);
        eventually(&stub.frees, 1);
        tagger.score(chunks("um hello"), soon()).unwrap();
        assert_eq!(stub.loads.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_model_that_is_not_installed_or_cannot_run_says_so() {
        let missing = Tagger::new(Stub {
            missing: true,
            ..Default::default()
        });
        assert_eq!(
            missing.score(chunks("um hello"), soon()),
            Err(Skip::ModelMissing)
        );
        // Asked again, it looks again.
        assert_eq!(
            missing.score(chunks("um hello"), soon()),
            Err(Skip::ModelMissing)
        );

        let failing = Tagger::new(Stub {
            fails: true,
            ..Default::default()
        });
        assert_eq!(
            failing.score(chunks("um hello"), soon()),
            Err(Skip::Failed("Core ML could not run the model".into()))
        );
    }

    #[test]
    fn a_late_answer_is_given_up_on_and_the_load_still_finishes() {
        let stub = Stub {
            load_time: Duration::from_millis(300),
            ..Default::default()
        };
        let tagger = Tagger::new(stub.clone());
        let started = Instant::now();
        let late = tagger.score(chunks("um hello"), started + Duration::from_millis(20));
        assert_eq!(late, Err(Skip::Late));
        assert!(started.elapsed() < Duration::from_millis(250));
        // The load finished behind the dictation: the next one is answered.
        eventually(&stub.loads, 1);
        assert!(tagger.score(chunks("um hello"), soon()).is_ok());
        assert_eq!(stub.loads.load(Ordering::SeqCst), 1);

        // A deadline already passed never waits.
        assert_eq!(
            tagger.score(chunks("um hello"), Instant::now()),
            Err(Skip::Late)
        );
    }
}
