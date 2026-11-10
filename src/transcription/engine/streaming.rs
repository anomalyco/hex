use super::*;

/// Feeds whatever part of the clip the stream has not seen, then finalizes.
/// The clip is authoritative: a stream fed well past its trimmed end, or one
/// that produced nothing, is reported unusable so the caller falls back.
/// The live stream as the worker drives it.
pub(super) trait StreamFeed {
    fn feed(&mut self, samples: &[f32]) -> Result<(), String>;
    fn finalize(&mut self) -> Result<String, String>;
    fn partial(&self) -> Partial;
    /// Where the stream's text starts in the audio it heard, in milliseconds,
    /// once it has finalized; `None` when the model gives no timing.
    fn text_start_ms(&self) -> Option<u64> {
        None
    }
    /// Where the stream's text ends in the audio it heard, in milliseconds,
    /// once it has finalized; `None` when the model gives no timing.
    fn text_end_ms(&self) -> Option<u64> {
        None
    }
    /// The distinct language tags the model's raw decode carries, in order of
    /// first appearance, once it has finalized; empty when it emits none.
    fn language_tags(&self) -> Vec<String> {
        Vec::new()
    }
}

impl StreamFeed for transcribe_cpp::Stream<'_> {
    fn feed(&mut self, samples: &[f32]) -> Result<(), String> {
        transcribe_cpp::Stream::feed(self, samples)
            .map(drop)
            .map_err(|error| error.to_string())
    }

    fn finalize(&mut self) -> Result<String, String> {
        transcribe_cpp::Stream::finalize(self).map_err(|error| error.to_string())?;
        let text = self.text().full.trim().to_string();
        // A subtitle credit the model appended at the end (#1141).
        Ok(crate::transcription::subtitle_credits::strip(&text).map_or(text, |(kept, _)| kept))
    }

    fn partial(&self) -> Partial {
        let text = self.text();
        Partial {
            stable: text.committed.trim_start().to_string(),
            tentative: text.tentative,
        }
    }

    fn text_start_ms(&self) -> Option<u64> {
        let snapshot = self.snapshot();
        if snapshot.timestamp_kind != transcribe_cpp::TimestampKind::Token {
            return None;
        }
        spoken_text_start_ms(&snapshot.tokens)
    }

    fn text_end_ms(&self) -> Option<u64> {
        let snapshot = self.snapshot();
        if snapshot.timestamp_kind != transcribe_cpp::TimestampKind::Token {
            return None;
        }
        spoken_text_end_ms(&snapshot.tokens)
    }

    fn language_tags(&self) -> Vec<String> {
        language_tags(&self.snapshot().raw_text)
            .into_iter()
            .map(str::to_string)
            .collect()
    }
}

/// Where the first token with letters or digits starts. A punctuation-only
/// token carries no word to hold against the speech before it.
pub(super) fn spoken_text_start_ms(tokens: &[transcribe_cpp::Token]) -> Option<u64> {
    tokens
        .iter()
        .filter(|token| token.text.chars().any(char::is_alphanumeric))
        .map(|token| token.t0_ms.max(0) as u64)
        .min()
}

/// The distinct language tags in a model's raw decode, in order of first
/// appearance. Nemotron's multilingual models emit one `<ll-RR>` tag per
/// segment, so two different tags mean the stream changed language mid-clip.
pub(super) fn language_tags(raw: &str) -> Vec<&str> {
    let bytes = raw.as_bytes();
    let mut tags: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match language_tag_len(&bytes[i..]) {
            Some(len) => {
                let tag = &raw[i..i + len];
                if !tags.contains(&tag) {
                    tags.push(tag);
                }
                i += len;
            }
            None => i += 1,
        }
    }
    tags
}

/// The length of the `<ll-RR>` tag at `bytes`'s start, as transcribe.cpp's
/// `is_lang_tag_piece` reads it: 2 or 3 lowercase letters, '-', 2 to 4
/// letters, and '>'.
fn language_tag_len(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 7 || bytes[0] != b'<' {
        return None;
    }
    let mut i = 1;
    let lang_start = i;
    while i < bytes.len() && bytes[i].is_ascii_lowercase() {
        i += 1;
    }
    if !(2..=3).contains(&(i - lang_start)) || bytes.get(i) != Some(&b'-') {
        return None;
    }
    i += 1;
    let region_start = i;
    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    if !(2..=4).contains(&(i - region_start)) || bytes.get(i) != Some(&b'>') {
        return None;
    }
    Some(i + 1)
}

/// Where the last token with letters or digits ends. Nemotron emits a
/// closing full stop at the clip's last frame, which would cover any phrase
/// the stream dropped before it.
pub(super) fn spoken_text_end_ms(tokens: &[transcribe_cpp::Token]) -> Option<u64> {
    tokens
        .iter()
        .filter(|token| token.text.chars().any(char::is_alphanumeric))
        .map(|token| token.t1_ms)
        .max()
        .filter(|&end| end > 0)
        .map(|end| end as u64)
}

/// Silence a dictation stream of `model` hears after the clip, before it
/// finalizes: the same trailing silence its whole-clip runs get, so a
/// streamed sentence keeps its closing punctuation (issue 657). Applies on
/// every OS.
pub(super) fn stream_trailing_silence(model: &Model) -> usize {
    model.stream_trailing_silence()
}

/// Pause audio a dictation stream still hears before its long pauses are
/// cut: longer than a chunk plus its lookahead (2.08 s for Unified English),
/// so the words before a pause decode during it.
pub(super) const PAUSE_HEAD_SAMPLES: usize = 40_000;
/// Pause audio kept just before speech resumes after a cut.
pub(super) const PAUSE_TAIL_SAMPLES: usize = 4_800;

/// Speech the stream heard may end this long after its text does and still
/// count as transcribed: a word's last token can be emitted before the word
/// fully ends.
pub(super) const UNTRANSCRIBED_SLACK_SAMPLES: usize = 6_400;
/// Speech heard after the text's end (and its slack) that makes the stream
/// unusable: a phrase the stream dropped (#1101). Longer than the stop
/// tone or key click the clip's 200 ms tail can carry.
pub(super) const UNTRANSCRIBED_SPEECH_SAMPLES: usize = 4_800;

/// A dictation stream that hears at most the head and tail of each pause.
/// Parakeet's buffered stream can drop the first word after a pause longer
/// than its 5.6 s left context (#324, #338); the clip, and with it any
/// offline fallback, keeps every sample.
///
/// It also checks the finalized text covers the speech the stream heard at
/// both ends: speech well before its start means the stream lost its opening,
/// speech well past its end means it dropped a phrase (#1101), and the whole
/// clip decides. A raw decode carrying two different language tags means it
/// changed language mid-clip, so the whole clip decides too.
pub(super) struct PauseCap<'a, S> {
    stream: &'a mut S,
    speech: crate::transcription::pause_segmenter::SpeechFrames,
    /// Samples not yet a whole 20 ms frame.
    frame: Vec<f32>,
    /// Samples passed to the stream so far.
    sent: usize,
    /// Where each speech frame the stream heard ends, in its samples.
    speech_ends: Vec<usize>,
    /// Quiet samples since the last frame above the noise floor.
    pause: usize,
    /// The newest pause samples past the head, released if speech resumes.
    tail: VecDeque<f32>,
    pub(super) dropped: usize,
    /// Whether any frame rose above the noise floor.
    heard: bool,
    /// Silence fed after the clip, just before finalizing.
    trailing_silence: usize,
}

impl<'a, S: StreamFeed> PauseCap<'a, S> {
    pub(super) fn new(stream: &'a mut S, trailing_silence: usize) -> Self {
        Self {
            stream,
            speech: Default::default(),
            frame: Vec::with_capacity(crate::transcription::pause_segmenter::FRAME_SAMPLES),
            sent: 0,
            speech_ends: Vec::new(),
            pause: 0,
            tail: VecDeque::with_capacity(PAUSE_TAIL_SAMPLES),
            dropped: 0,
            heard: false,
            trailing_silence,
        }
    }

    fn frame(&mut self, out: &mut Vec<f32>) {
        // Only audio at the noise floor counts as pause. Speech a few dB over
        // steady noise misses the meeting detector's speech margin, and
        // cutting it truncated the transcript (#458).
        let level = self.speech.classify(&self.frame);
        if !level.quiet {
            self.heard = true;
            out.extend(self.tail.drain(..));
            self.pause = 0;
            out.append(&mut self.frame);
            if level.speech {
                self.speech_ends.push(self.sent + out.len());
            }
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
        self.sent += out.len();
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
        // Everything after the first pause head was cut, yet the clip reached
        // the transcriber: the cap misjudged the microphone's level, so the
        // stream heard only the opening words (#841). The whole clip decides.
        if !self.heard && self.dropped > 0 {
            return Err(format!(
                "the stream heard only quiet audio and {} ms was cut as a pause",
                self.dropped / 16
            ));
        }
        if self.trailing_silence > 0 {
            self.stream.feed(&vec![0.0; self.trailing_silence])?;
        }
        let text = self.stream.finalize()?;
        if !text.is_empty()
            && let Some(start_ms) = self.stream.text_start_ms()
        {
            // The trailing check's slack and threshold, on the speech that
            // ends before the text begins. Unlike the trailing check, a
            // late-emitted first token moves t0 later, the unsafe direction
            // here, so the cost is at most a whole-clip run.
            let covered = (start_ms as usize * 16).saturating_sub(UNTRANSCRIBED_SLACK_SAMPLES);
            let untranscribed = self
                .speech_ends
                .iter()
                .filter(|&&end| end < covered)
                .count()
                * crate::transcription::pause_segmenter::FRAME_SAMPLES;
            if untranscribed >= UNTRANSCRIBED_SPEECH_SAMPLES {
                return Err(format!(
                    "the stream's text starts at {start_ms} ms but {} ms of speech precedes it",
                    untranscribed / 16
                ));
            }
        }
        if !text.is_empty()
            && let Some(end_ms) = self.stream.text_end_ms()
        {
            let covered = end_ms as usize * 16 + UNTRANSCRIBED_SLACK_SAMPLES;
            let untranscribed = self
                .speech_ends
                .iter()
                .filter(|&&end| end > covered)
                .count()
                * crate::transcription::pause_segmenter::FRAME_SAMPLES;
            if untranscribed >= UNTRANSCRIBED_SPEECH_SAMPLES {
                return Err(format!(
                    "the stream's text ends at {end_ms} ms but {} ms of speech follows",
                    untranscribed / 16
                ));
            }
        }
        if !text.is_empty()
            && let [from, to, ..] = self.stream.language_tags().as_slice()
        {
            return Err(format!("the stream switched language from {from} to {to}"));
        }
        Ok(text)
    }

    fn partial(&self) -> Partial {
        self.stream.partial()
    }
}

/// Collects what a [`PauseCap`] lets through, for a whole clip.
#[derive(Default)]
#[cfg_attr(target_os = "macos", allow(dead_code))]
struct Kept(Vec<f32>);

impl StreamFeed for Kept {
    fn feed(&mut self, samples: &[f32]) -> Result<(), String> {
        self.0.extend_from_slice(samples);
        Ok(())
    }

    fn finalize(&mut self) -> Result<String, String> {
        Ok(String::new())
    }

    fn partial(&self) -> Partial {
        Partial::default()
    }
}

/// A whole 16 kHz clip with each long pause cut to its head and tail, as a
/// dictation stream hears it, and the samples dropped. `None` keeps the
/// clip whole: nothing was long enough to cut, or no frame rose above the
/// noise floor yet audio would have been cut (#841).
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(super) fn skip_long_pauses(clip: &[f32]) -> Option<(Vec<f32>, usize)> {
    let mut kept = Kept::default();
    let mut capped = PauseCap::new(&mut kept, 0);
    capped.feed(clip).ok()?;
    capped.finalize().ok()?;
    let dropped = capped.dropped;
    (dropped > 0).then_some((kept.0, dropped))
}

/// The clip a whole-clip dictation run transcribes: with Labs › Skip long
/// pauses (`skip_pauses`), its long pauses cut; otherwise exactly `clip`.
/// Job deadlines, History, and the silence gate keep the full clip.
#[cfg_attr(target_os = "macos", allow(dead_code))]
pub(super) fn whole_clip(clip: Vec<f32>, skip_pauses: bool) -> Vec<f32> {
    if !skip_pauses {
        return clip;
    }
    match skip_long_pauses(&clip) {
        Some((kept, dropped)) => {
            tracing::info!(
                audio_ms = clip.len() / 16,
                pauses_dropped_ms = dropped / 16,
                "skipped long pauses before whole-clip transcription"
            );
            kept
        }
        None => clip,
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
