use super::*;

/// Feeds whatever part of the clip the stream has not seen, then finalizes.
/// The clip is authoritative: a stream fed well past its trimmed end, or one
/// that produced nothing, is reported unusable so the caller falls back.
/// The live stream as the worker drives it.
pub(super) trait StreamFeed {
    fn feed(&mut self, samples: &[f32]) -> Result<(), String>;
    fn finalize(&mut self) -> Result<String, String>;
    fn partial(&self) -> Partial;
}

impl StreamFeed for transcribe_cpp::Stream<'_> {
    fn feed(&mut self, samples: &[f32]) -> Result<(), String> {
        transcribe_cpp::Stream::feed(self, samples)
            .map(drop)
            .map_err(|error| error.to_string())
    }

    fn finalize(&mut self) -> Result<String, String> {
        transcribe_cpp::Stream::finalize(self).map_err(|error| error.to_string())?;
        Ok(self.text().full.trim().to_string())
    }

    fn partial(&self) -> Partial {
        let text = self.text();
        Partial {
            stable: text.committed.trim_start().to_string(),
            tentative: text.tentative,
        }
    }
}

/// Silence a dictation stream of `model` hears after the clip, before it
/// finalizes: on macOS the trailing silence its whole-clip runs get, so a
/// streamed sentence keeps its closing punctuation (issue 657).
#[cfg(target_os = "macos")]
pub(super) fn stream_trailing_silence(model: &Model) -> usize {
    model.stream_trailing_silence()
}

#[cfg(not(target_os = "macos"))]
pub(super) fn stream_trailing_silence(_model: &Model) -> usize {
    0
}

/// Pause audio a dictation stream still hears before its long pauses are
/// cut: longer than a chunk plus its lookahead (2.08 s for Unified English),
/// so the words before a pause decode during it.
pub(super) const PAUSE_HEAD_SAMPLES: usize = 40_000;
/// Pause audio kept just before speech resumes after a cut.
pub(super) const PAUSE_TAIL_SAMPLES: usize = 4_800;

/// A dictation stream that hears at most the head and tail of each pause.
/// Parakeet's buffered stream can drop the first word after a pause longer
/// than its 5.6 s left context (#324, #338); the clip, and with it any
/// offline fallback, keeps every sample.
pub(super) struct PauseCap<'a, S> {
    stream: &'a mut S,
    speech: crate::transcription::pause_segmenter::SpeechFrames,
    /// Samples not yet a whole 20 ms frame.
    frame: Vec<f32>,
    /// Quiet samples since the last frame above the noise floor.
    pause: usize,
    /// The newest pause samples past the head, released if speech resumes.
    tail: VecDeque<f32>,
    pub(super) dropped: usize,
    /// Silence fed after the clip, just before finalizing.
    trailing_silence: usize,
}

impl<'a, S: StreamFeed> PauseCap<'a, S> {
    pub(super) fn new(stream: &'a mut S, trailing_silence: usize) -> Self {
        Self {
            stream,
            speech: Default::default(),
            frame: Vec::with_capacity(crate::transcription::pause_segmenter::FRAME_SAMPLES),
            pause: 0,
            tail: VecDeque::with_capacity(PAUSE_TAIL_SAMPLES),
            dropped: 0,
            trailing_silence,
        }
    }

    fn frame(&mut self, out: &mut Vec<f32>) {
        // Only audio at the noise floor counts as pause. Speech a few dB over
        // steady noise misses the meeting detector's speech margin, and
        // cutting it truncated the transcript (#458).
        if !self.speech.is_quiet(&self.frame) {
            out.extend(self.tail.drain(..));
            self.pause = 0;
            out.append(&mut self.frame);
            return;
        }
        self.pause += self.frame.len();
        if self.pause <= PAUSE_HEAD_SAMPLES {
            out.append(&mut self.frame);
            return;
        }
        self.tail.extend(self.frame.drain(..));
        let overflow = self.tail.len().saturating_sub(PAUSE_TAIL_SAMPLES);
        self.tail.drain(..overflow);
        self.dropped += overflow;
    }
}

impl<S: StreamFeed> StreamFeed for PauseCap<'_, S> {
    fn feed(&mut self, mut samples: &[f32]) -> Result<(), String> {
        let mut out = Vec::with_capacity(samples.len() + self.tail.len());
        while !samples.is_empty() {
            let take = samples
                .len()
                .min(crate::transcription::pause_segmenter::FRAME_SAMPLES - self.frame.len());
            self.frame.extend_from_slice(&samples[..take]);
            samples = &samples[take..];
            if self.frame.len() == crate::transcription::pause_segmenter::FRAME_SAMPLES {
                self.frame(&mut out);
            }
        }
        if out.is_empty() {
            return Ok(());
        }
        self.stream.feed(&out)
    }

    /// A trailing pause keeps only its head; then the model's trailing
    /// silence follows.
    fn finalize(&mut self) -> Result<String, String> {
        if self.pause <= PAUSE_HEAD_SAMPLES && !self.frame.is_empty() {
            let rest = std::mem::take(&mut self.frame);
            self.stream.feed(&rest)?;
        } else {
            self.dropped += self.frame.len();
        }
        self.dropped += self.tail.len();
        self.tail.clear();
        if self.trailing_silence > 0 {
            self.stream.feed(&vec![0.0; self.trailing_silence])?;
        }
        self.stream.finalize()
    }

    fn partial(&self) -> Partial {
        self.stream.partial()
    }
}

#[derive(Default)]
pub(super) struct FeedStats {
    pub(super) feed_ms: u128,
    pub(super) feeds: u32,
    /// Changes of text published for live text.
    pub(super) partials: u32,
    /// Pause audio the stream never heard.
    pub(super) pauses_dropped_ms: usize,
}

/// Handles commands while stream `id` is open, publishing its text to
/// `partials` after each feed when live text is on. Foreground work waits in
/// `deferred` until the stream ends; background work is skipped; any other
/// command interrupts the stream.
pub(super) fn drive_stream(
    id: u64,
    stream: &mut impl StreamFeed,
    commands: &Receiver<Command>,
    stalled: &AtomicBool,
    partials: Option<&Partials>,
    deferred: &mut VecDeque<Command>,
    stats: &mut FeedStats,
) -> StreamEnd {
    let mut failure: Option<String> = None;
    let mut fed = 0_usize;
    loop {
        let Ok(command) = commands.recv() else {
            return StreamEnd::Disconnected;
        };
        match command {
            Command::Feed { stream: s, samples } if s == id => {
                fed += samples.len();
                if failure.is_none() {
                    let started = Instant::now();
                    match stream.feed(&samples) {
                        Ok(()) => {
                            if partials.is_some_and(|partials| partials.publish(stream.partial())) {
                                stats.partials += 1;
                            }
                        }
                        Err(error) => {
                            failure = Some(format!("stream feed failed: {error}"));
                            // The text stops here; the plate hides rather
                            // than freeze on it while the capture goes on.
                            if let Some(partials) = partials {
                                partials.publish(Partial::default());
                            }
                        }
                    }
                    stats.feed_ms += started.elapsed().as_millis();
                    stats.feeds += 1;
                }
            }
            Command::Feed { .. } => {}
            // Background work never interrupts a capture.
            Command::Background { work, .. } => work(None),
            // Nor does foreground work; it runs once the stream ends.
            foreground @ Command::Foreground { .. } => deferred.push_back(foreground),
            // Nor does a stream that gives way to dictation; it is refused.
            Command::BeginYielding { yielding, .. } => yielding.close(),
            Command::FinishYielding { reply, .. } => {
                let _ = reply.send(Err(PREEMPTED.into()));
            }
            Command::Cancel { stream: s } if s == id => return StreamEnd::Cancelled,
            Command::Cancel { .. } => {}
            Command::Finish {
                stream: Some(s),
                clip,
                offline,
                job,
            } if s == id => {
                let started = Instant::now();
                stalled.store(false, Ordering::Release);
                job.start();
                let result = match failure {
                    Some(failure) => Err(failure),
                    None => finish_stream(stream, &clip, fed),
                };
                return StreamEnd::Finished {
                    result,
                    clip,
                    offline,
                    job,
                    started,
                };
            }
            other => return StreamEnd::Interrupted(other),
        }
    }
}

/// Handles commands while yielding stream `id` is open, publishing its
/// partial text after each feed. Background work is skipped and foreground
/// work deferred, as in a dictation stream; any other command (a dictation
/// begin or finish, model work, shutdown) closes it and is returned.
pub(super) fn drive_yielding(
    id: u64,
    stream: &mut impl StreamFeed,
    commands: &Receiver<Command>,
    yielding: &Yielding,
    deferred: &mut VecDeque<Command>,
) -> Option<Command> {
    loop {
        let Ok(command) = commands.recv() else {
            return None;
        };
        match command {
            Command::Feed { stream: s, samples } if s == id => match stream.feed(&samples) {
                Ok(()) => {
                    yielding.consumed.fetch_add(samples.len(), Ordering::AcqRel);
                    yielding.partials.publish(stream.partial());
                }
                Err(error) => {
                    tracing::warn!(%error, "yielding stream feed failed");
                    return None;
                }
            },
            Command::Feed { .. } => {}
            Command::Background { work, .. } => work(None),
            foreground @ Command::Foreground { .. } => deferred.push_back(foreground),
            Command::Cancel { stream: s } if s == id => return None,
            Command::Cancel { .. } => {}
            Command::FinishYielding { stream: s, reply } if s == id => {
                let _ = reply.send(stream.finalize());
                return None;
            }
            Command::FinishYielding { reply, .. } => {
                let _ = reply.send(Err(PREEMPTED.into()));
            }
            Command::BeginYielding { yielding, .. } => yielding.close(),
            other => {
                yielding.close();
                return Some(other);
            }
        }
    }
}

pub(super) fn finish_stream(
    stream: &mut impl StreamFeed,
    clip: &[f32],
    fed: usize,
) -> Result<String, String> {
    if fed > clip.len() + MAX_OVERFED_SAMPLES {
        return Err(format!(
            "the stream received {} ms past the clip's end",
            (fed - clip.len()) / 16
        ));
    }
    if clip.len() > fed {
        stream
            .feed(&clip[fed..])
            .map_err(|error| format!("stream feed failed: {error}"))?;
    }
    let text = stream
        .finalize()
        .map_err(|error| format!("stream finalize failed: {error}"))?;
    if text.is_empty() {
        return Err("the stream returned no text".into());
    }
    Ok(text)
}
