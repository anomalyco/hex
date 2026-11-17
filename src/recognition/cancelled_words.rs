//! A cancel has exactly one possible destination: retained local words. No
//! paste worker, context, script, recorder or route is available here.
use crate::history::History;
use crate::transcription::{engine::TranscriptionEngine, speech_detector};
use std::sync::{
    Arc,
    mpsc::{self, SyncSender},
};

pub(crate) struct Writer {
    history: History,
    queue: SyncSender<(u64, Vec<f32>)>,
    #[cfg(test)]
    thread: std::thread::JoinHandle<()>,
}
impl Writer {
    pub(crate) fn start(history: History, engine: Arc<TranscriptionEngine>) -> Option<Self> {
        let (queue, rx) = mpsc::sync_channel::<(u64, Vec<f32>)>(2);
        let owner = history.clone();
        let _thread = std::thread::Builder::new()
            .name("cancelled-words".into())
            .spawn(move || {
                while let Ok((generation, samples)) = rx.recv() {
                    if owner.cancel_generation() != Some(generation)
                        || crate::desktop::process_exit::quitting()
                    {
                        continue;
                    }
                    if let Err(error) = recover(&owner, &engine, generation, samples) {
                        tracing::warn!(%error,"could not recover cancelled words locally");
                    }
                }
            })
            .ok()?;
        Some(Self {
            history,
            queue,
            #[cfg(test)]
            thread: _thread,
        })
    }
    /// Called only for the physical cancel decision of a shortcut recording.
    pub(crate) fn cancel(
        &self,
        input: &crate::dictation::audio::DictationAudio,
        at: crate::audio::CaptureInstant,
    ) -> color_eyre::Result<()> {
        let samples = input.cancel_spoken(at)?;
        if !samples.is_empty()
            && let Some(generation) = self.history.cancel_generation()
        {
            let _ = self.queue.try_send((generation, samples));
        }
        Ok(())
    }
    #[cfg(test)]
    pub(crate) fn drain(self) {
        drop(self.queue);
        self.thread.join().unwrap();
    }
}

fn recover(
    owner: &History,
    engine: &TranscriptionEngine,
    generation: u64,
    samples: Vec<f32>,
) -> color_eyre::Result<()> {
    if samples.is_empty()
        || samples.iter().all(|&s| s == 0.)
        || !speech_detector::installed_speech(&samples)
    {
        return Ok(());
    }
    let duration = samples.len() as u64 / 16;
    let transcript = engine
        .foreground(
            samples,
            Box::new(|model, clip| {
                model
                    .active()
                    .ok_or("no local speech model")?
                    .transcribe(clip)
                    .map_err(|e| e.to_string())
            }),
        )?
        .wait()
        .map_err(|e| color_eyre::eyre::eyre!(e))?;
    owner.keep_cancelled_words(generation, transcript.text, duration)?;
    Ok(())
}
