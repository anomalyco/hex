//! Starting a plan on `ScreenCaptureKit`: one stream and one recording output per source.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::Duration;

use screencapturekit::audio_devices::AudioInputDevice;
use screencapturekit::cg::CGRect;
use screencapturekit::cm::{CMSampleBuffer, CMSampleBufferSCExt, CMTime};
use screencapturekit::recording_output::{
    RecordingCallbacks, SCRecordingOutput, SCRecordingOutputCodec, SCRecordingOutputConfiguration,
    SCRecordingOutputFileType,
};
use screencapturekit::shareable_content::{
    SCDisplay, SCRunningApplication, SCShareableContent, SCWindow,
};
use screencapturekit::stream::SCStream;
use screencapturekit::stream::configuration::{PixelFormat, SCStreamConfiguration};
use screencapturekit::stream::content_filter::SCContentFilter;
use screencapturekit::stream::delegate_trait::StreamCallbacks;
use screencapturekit::stream::output_type::SCStreamOutputType;

use crate::clicks::ClickMonitor;
use crate::cursor::{
    Collectors, CursorClickCaptureStatus, CursorCollector, PointerHistory, PointerSampler,
};
use crate::error::Error;
use crate::external_audio::ExternalAudioWriter;
use crate::frames::FrameSampler;
use crate::input::{HostInputDrain, InputLog};
use crate::manifest::{Permission, TrackKind};
use crate::plan::{
    CursorDrawing, Exclusions, Input, Mic, Plan, Rect, Source, SourcePlan, output_size,
};
use crate::session::{
    Active, Recording, RecordingEvent, RecordingEvents, SourceRun, SourceStream, Stopper,
    validate_sources,
};
use crate::system_audio::SystemAudioWriter;
use crate::time::{HostTime, MediaTime};

/// `ScreenCaptureKit` can need over a second to start a selected Core Audio device. The first
/// source's recording output is attached only after that, so the MP4's audio and video tracks
/// share an origin instead of opening with a long silent gap.

/// Starts recordings.
pub struct Recorder;

impl Recorder {
    /// Starts every source in the plan. The first source owns audio and is started last, so a
    /// microphone's warm-up does not delay the other sources.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid plan, a missing source, device or permission, a name that
    /// already exists in the directory, or a `ScreenCaptureKit` failure. A failed start removes
    /// every file it created.
    pub fn start(plan: Plan) -> Result<Recording, Error> {
        let Plan {
            sources,
            cursor,
            audio,
            input,
            frames,
            exclude,
            fps,
            dir,
            names,
        } = plan;
        if fps == 0 {
            return Err(Error::InvalidFrameRate);
        }
        validate_sources(&sources.iter().map(|plan| plan.source).collect::<Vec<_>>())?;
        names.validate(sources.len(), &audio)?;
        check_dir(&dir, names.all().chain([names.manifest_name()]))?;
        if let Mic::Device { id, .. } = &audio.mic
            && !AudioInputDevice::list()
                .iter()
                .any(|device| &device.id == id)
        {
            return Err(Error::MicrophoneUnavailable(id.clone()));
        }
        let content = SCShareableContent::get()
            .map_err(|error| Error::ShareableContent(error.to_string()))?;
        let targets = sources
            .iter()
            .map(|source| Target::find(&content, *source, &exclude))
            .collect::<Result<Vec<_>, _>>()?;

        let anchor = HostTime::now();
        let mut active = Active::new(dir.clone(), names.clone(), anchor);
        let telemetry = names.cursor.is_some();
        let collectors = targets
            .iter()
            .map(|target| {
                Arc::new(Mutex::new(CursorCollector::for_capture(
                    target.rect,
                    telemetry,
                )))
            })
            .collect::<Vec<_>>();
        let started = (|| {
            if telemetry {
                active.pointer = Some(PointerSampler::start().map_err(|error| {
                    Error::Start(format!("could not start the pointer sampler: {error}"))
                })?);
            }
            let mut external_sink = None;
            if let (Mic::External(config), Some(name)) = (&audio.mic, &names.microphone) {
                let (writer, sink) = ExternalAudioWriter::start(&dir.join(name), anchor, *config)?;
                active.external = Some(writer);
                external_sink = Some(sink);
            }
            if let (true, Some(name)) = (audio.system, &names.system_audio) {
                active.system_audio = Some(Arc::new(
                    SystemAudioWriter::create(&dir.join(name)).map_err(Error::ConfigureStream)?,
                ));
            }
            active.frames = frames
                .iter()
                .flat_map(|tap| {
                    (0..targets.len()).map(|index| Arc::new(FrameSampler::new(tap.clone(), index)))
                })
                .collect();

            let mut runs = std::iter::repeat_with(|| None)
                .take(targets.len())
                .collect::<Vec<Option<SourceRun>>>();
            let start_order = (1..targets.len()).chain(std::iter::once(0));
            for index in start_order {
                let context = SourceContext {
                    source: sources[index].source,
                    region: targets[index].source_rect.map(|_| targets[index].rect),
                    fps,
                    cursor,
                    telemetry,
                    microphone: match (&audio.mic, index) {
                        (Mic::Device { id, warmup }, 0) => Some((id.as_str(), *warmup)),
                        _ => None,
                    },
                    system_audio: audio.system && index == 0,
                    system_writer: (index == 0).then(|| active.system_audio.clone()).flatten(),
                    collector: Arc::clone(&collectors[index]),
                    pointer: active.pointer.as_ref().map(PointerSampler::history),
                    frames: active.frames.get(index).cloned(),
                    stopping: Arc::clone(&active.stopping),
                    name: names.videos[index].clone(),
                    path: dir.join(&names.videos[index]),
                };
                match start_source(&targets[index], context) {
                    Ok(run) => runs[index] = Some(run),
                    Err(error) => {
                        active.sources = runs.into_iter().flatten().collect();
                        return Err(error);
                    }
                }
            }
            active.sources = runs.into_iter().flatten().collect();

            let log = (!matches!(input, Input::None))
                .then(|| InputLog::new(anchor, Collectors(collectors.clone())));
            match (input, &log) {
                (Input::Host(receiver), Some(log)) => {
                    log.collectors()
                        .set_click_status(CursorClickCaptureStatus::Active);
                    active.host_input = Some(
                        HostInputDrain::start(receiver, Arc::clone(log)).map_err(|error| {
                            Error::Start(format!("could not start the input drain: {error}"))
                        })?,
                    );
                }
                (Input::Tap, Some(log)) => {
                    let monitor = ClickMonitor::start(Arc::clone(log));
                    if monitor.status() == CursorClickCaptureStatus::PermissionDenied {
                        active.missing_permissions.push(Permission::InputMonitoring);
                    }
                    active.clicks = Some(monitor);
                }
                _ => {}
            }
            active.input_log = log;
            Ok(external_sink)
        })();
        match started {
            Ok(external_sink) => Ok(Recording {
                active: Some(active),
                external_sink,
            }),
            Err(error) => {
                tracing::warn!(%error, "recording did not start");
                drop(active.finish(HostTime::now(), true));
                Err(error)
            }
        }
    }
}

fn check_dir<'a>(dir: &Path, names: impl Iterator<Item = &'a str>) -> Result<(), Error> {
    if !dir.is_dir() {
        return Err(Error::Output(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} is not a directory", dir.display()),
        )));
    }
    for name in names {
        let path = dir.join(name);
        if path.exists() {
            return Err(Error::Output(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("{} already exists", path.display()),
            )));
        }
    }
    Ok(())
}

/// A source resolved against the current shareable content.
pub(crate) struct Target {
    pub filter: SCContentFilter,
    /// Human-readable label captured when the source opened, if any.
    pub label: Option<String>,
    /// Encoded size in pixels.
    pub width: u32,
    pub height: u32,
    /// Backing pixels per point when the source opened.
    pub scale: Option<f64>,
    /// The captured area in global top-left points.
    pub rect: Rect,
    /// For a region, the area in its display's own points.
    pub source_rect: Option<Rect>,
}

impl Target {
    pub(crate) fn find(
        content: &SCShareableContent,
        plan: SourcePlan,
        exclude: &Exclusions,
    ) -> Result<Self, Error> {
        let source = plan.source;
        let (filter, label, rect, source_rect) = match source {
            Source::Display(id) => {
                let display = find_display(content, id).ok_or(Error::SourceUnavailable(source))?;
                let filter = display_filter(content, &display, exclude)?;
                (filter, None, rect_of(display.frame()), None)
            }
            Source::Window(id) => {
                let window = content
                    .windows()
                    .into_iter()
                    .find(|window| window.window_id() == id)
                    .ok_or(Error::SourceUnavailable(source))?;
                let label = window_label(&window);
                let filter = SCContentFilter::create()
                    .with_window(&window)
                    .build()
                    .map_err(|error| Error::ConfigureStream(error.to_string()))?;
                (filter, label, rect_of(window.frame()), None)
            }
            Source::Region { display: id, rect } => {
                let display = find_display(content, id).ok_or(Error::SourceUnavailable(source))?;
                let frame = rect_of(display.frame());
                let local = rect
                    .intersection(Rect {
                        x: 0.0,
                        y: 0.0,
                        width: frame.width,
                        height: frame.height,
                    })
                    .ok_or(Error::InvalidRegion(rect))?;
                let filter = display_filter(content, &display, exclude)?;
                let global = Rect {
                    x: frame.x + local.x,
                    y: frame.y + local.y,
                    ..local
                };
                (filter, None, global, Some(local))
            }
        };
        let scale = f64::from(filter.point_pixel_scale());
        let (width, height) = output_size((rect.width, rect.height), scale, plan.long_edge)
            .ok_or(Error::InvalidDimensions)?;
        Ok(Self {
            filter,
            label,
            width,
            height,
            scale: (scale.is_finite() && scale > 0.0).then_some(scale),
            rect,
            source_rect,
        })
    }

    /// The stream configuration's size and area for this target.
    pub(crate) fn configure(&self, config: SCStreamConfiguration) -> SCStreamConfiguration {
        let config = config.with_width(self.width).with_height(self.height);
        match self.source_rect {
            Some(rect) => {
                config.with_source_rect(CGRect::new(rect.x, rect.y, rect.width, rect.height))
            }
            None => config,
        }
    }
}

fn find_display(content: &SCShareableContent, id: u32) -> Option<SCDisplay> {
    content
        .displays()
        .into_iter()
        .find(|display| display.display_id() == id)
}

fn rect_of(frame: CGRect) -> Rect {
    Rect {
        x: frame.origin.x,
        y: frame.origin.y,
        width: frame.size.width,
        height: frame.size.height,
    }
}

fn window_label(window: &SCWindow) -> Option<String> {
    let app = window
        .owning_application()
        .map(|app| app.application_name().trim().to_owned())
        .filter(|name| !name.is_empty());
    let title = window
        .title()
        .map(|title| title.trim().to_owned())
        .filter(|title| !title.is_empty());
    match (app, title) {
        (Some(app), Some(title)) if app != title => Some(format!("{app} — {title}")),
        (Some(label), _) | (None, Some(label)) => Some(label),
        (None, None) => None,
    }
}

/// A display filter leaving out exactly what the host asked for. Windows or apps no longer on
/// screen are skipped.
fn display_filter(
    content: &SCShareableContent,
    display: &SCDisplay,
    exclude: &Exclusions,
) -> Result<SCContentFilter, Error> {
    let windows = content
        .windows()
        .into_iter()
        .filter(|window| exclude.windows.contains(&window.window_id()))
        .collect::<Vec<SCWindow>>();
    let windows = windows.iter().collect::<Vec<_>>();
    let builder = SCContentFilter::create().with_display(display);
    let builder = if exclude.apps.is_empty() {
        builder.with_excluding_windows(&windows)
    } else {
        // A display filter excludes windows or apps, not both: every excluded window must belong
        // to an excluded app.
        if let Some(window) = windows.iter().find(|window| {
            !window
                .owning_application()
                .is_some_and(|app| exclude.apps.contains(&app.process_id()))
        }) {
            return Err(Error::Exclusion(format!(
                "window {} does not belong to an excluded app",
                window.window_id()
            )));
        }
        let apps = content
            .applications()
            .into_iter()
            .filter(|app| exclude.apps.contains(&app.process_id()))
            .collect::<Vec<SCRunningApplication>>();
        builder.with_excluding_applications(&apps.iter().collect::<Vec<_>>(), &[])
    };
    builder
        .build()
        .map_err(|error| Error::Exclusion(error.to_string()))
}

struct SourceContext<'a> {
    source: Source,
    /// A region's area in global points. It places the cursor, whatever rect its frames report.
    region: Option<Rect>,
    fps: u32,
    cursor: CursorDrawing,
    telemetry: bool,
    /// The device and its warm-up.
    microphone: Option<(&'a str, Duration)>,
    system_audio: bool,
    system_writer: Option<Arc<SystemAudioWriter>>,
    collector: Arc<Mutex<CursorCollector>>,
    pointer: Option<Arc<Mutex<PointerHistory>>>,
    frames: Option<Arc<FrameSampler>>,
    stopping: Arc<AtomicBool>,
    name: String,
    path: PathBuf,
}

fn start_source(target: &Target, context: SourceContext<'_>) -> Result<SourceRun, Error> {
    let configure =
        |error: screencapturekit::error::SCError| Error::ConfigureStream(error.to_string());
    let interval = CMTime::new(1, i32::try_from(context.fps).unwrap_or(i32::MAX));
    let mut config = target
        .configure(SCStreamConfiguration::new())
        .with_minimum_frame_interval(&interval)
        .with_queue_depth(8)
        .with_pixel_format(PixelFormat::BGRA)
        .with_shows_cursor(context.cursor == CursorDrawing::Baked)
        .with_captures_audio(context.system_audio)
        .with_excludes_current_process_audio(true)
        .with_sample_rate(48_000)
        .with_channel_count(2)
        .with_shows_mouse_clicks(false)
        .map_err(configure)?
        .with_ignores_shadows_single_window(context.telemetry)
        .map_err(configure)?
        // Off unless the plan names a device: the core never opens a microphone on its own.
        .with_captures_microphone(context.microphone.is_some())
        .map_err(configure)?;
    if let Some((id, _)) = context.microphone {
        config = config
            .with_microphone_capture_device_id(id)
            .map_err(configure)?;
    }

    let recording_config = SCRecordingOutputConfiguration::new()
        .map_err(|error| Error::ConfigureRecording(error.to_string()))?
        .with_output_url(&context.path)
        .map_err(|error| Error::ConfigureRecording(error.to_string()))?
        .with_video_codec(SCRecordingOutputCodec::H264)
        .with_output_file_type(SCRecordingOutputFileType::MP4);
    let (sender, receiver) = mpsc::channel();
    let output =
        SCRecordingOutput::new_with_delegate(&recording_config, recording_callbacks(&sender))
            .ok_or(Error::CreateRecordingOutput)?;
    let stream_stopping = Arc::clone(&context.stopping);
    let stream_callbacks = StreamCallbacks::new().on_stop(move |error| {
        if stream_stopping.load(Ordering::Acquire) {
            return;
        }
        let error = error.unwrap_or_else(|| "capture stream stopped unexpectedly".into());
        let _ = sender.send(RecordingEvent::StreamStopped(error));
    });
    let stream = SCStream::new_with_delegate(&target.filter, &config, stream_callbacks)
        .map_err(|error| Error::Start(error.to_string()))?;
    let mut native = NativeStream {
        source: context.source,
        stream,
        output,
        handlers: Vec::new(),
    };
    if let Err(error) = native.add_handlers(&context) {
        let _ = native.remove_handlers();
        return Err(error);
    }

    let attach = |native: &NativeStream| -> Result<HostTime, Error> {
        native
            .stream
            .add_recording_output(&native.output)
            .map_err(|error| Error::ConfigureRecording(error.to_string()))?;
        let attached = HostTime::now();
        context
            .collector
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .arm(attached.ticks());
        Ok(attached)
    };
    let mut attached = None;
    if context.microphone.is_none() {
        match attach(&native) {
            Ok(at) => attached = Some(at),
            Err(error) => {
                let _ = native.remove_handlers();
                return Err(error);
            }
        }
    }
    if let Err(error) = native.stream.start_capture() {
        native.abandon(attached.is_some());
        return Err(Error::Start(error.to_string()));
    }
    let attached = match attached {
        Some(at) => at,
        None => {
            if let Some((_, warmup)) = context.microphone {
                thread::sleep(warmup);
            }
            match attach(&native) {
                Ok(at) => at,
                Err(error) => {
                    native.abandon(false);
                    return Err(error);
                }
            }
        }
    };

    let mut audio_tracks = Vec::new();
    if context.system_audio {
        audio_tracks.push(TrackKind::SystemAudio);
    }
    if context.microphone.is_some() {
        audio_tracks.push(TrackKind::Microphone);
    }
    Ok(SourceRun {
        source: context.source,
        label: target.label.clone(),
        name: context.name,
        path: context.path,
        stream: Box::new(native),
        events: RecordingEvents::new(receiver),
        collector: context.collector,
        attached,
        width: target.width,
        height: target.height,
        scale: target.scale,
        rect: target.rect,
        audio_tracks,
    })
}

fn recording_callbacks(sender: &Sender<RecordingEvent>) -> RecordingCallbacks {
    let start = sender.clone();
    let fail = sender.clone();
    let finish = sender.clone();
    RecordingCallbacks::new()
        .on_start(move || {
            let _ = start.send(RecordingEvent::Started);
        })
        .on_fail(move |error| {
            let _ = fail.send(RecordingEvent::Failed(error));
        })
        .on_finish(move || {
            let _ = finish.send(RecordingEvent::Finished);
        })
}

struct NativeStream {
    source: Source,
    stream: SCStream,
    output: SCRecordingOutput,
    handlers: Vec<(usize, SCStreamOutputType)>,
}

impl NativeStream {
    /// A frame handler is always attached: it timestamps the source's first frame, and while
    /// it stays attached, removing the recording output never stops the stream on its own.
    fn add_handlers(&mut self, context: &SourceContext<'_>) -> Result<(), Error> {
        let collector = Arc::clone(&context.collector);
        let region = context.region;
        let pointer = context.pointer.clone();
        let frames = context.frames.clone();
        let stopping = Arc::clone(&context.stopping);
        let screen = self
            .stream
            .add_output_handler(
                move |sample: CMSampleBuffer, _| {
                    let info = sample.frame_info().unwrap_or_default();
                    let screen_rect = region.or_else(|| info.screen_rect.map(rect_of));
                    let point = info.display_time.and_then(|display_time| {
                        pointer.as_ref().and_then(|history| {
                            history
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .point_at(display_time)
                        })
                    });
                    collector
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .observe_cm_frame(
                            info.frame_status,
                            sample.presentation_timestamp(),
                            info.display_time,
                            screen_rect,
                            point,
                        );
                    if let Some(frames) = &frames
                        && !stopping.load(Ordering::Acquire)
                    {
                        frames.observe(sample, info.frame_status, info.display_time);
                    }
                },
                SCStreamOutputType::Screen,
            )
            .map_err(|error| Error::ConfigureStream(error.to_string()))?;
        self.handlers.push((screen, SCStreamOutputType::Screen));
        if let Some(writer) = context.system_writer.clone() {
            let audio = self
                .stream
                .add_output_handler(
                    move |sample: CMSampleBuffer, _| writer.append(&sample),
                    SCStreamOutputType::Audio,
                )
                .map_err(|error| Error::ConfigureStream(error.to_string()))?;
            self.handlers.push((audio, SCStreamOutputType::Audio));
        }
        Ok(())
    }

    /// Unwinds a source that failed to start.
    fn abandon(&mut self, output_attached: bool) {
        if output_attached {
            let _ = self.stream.remove_recording_output(&self.output);
        }
        let _ = self.remove_handlers();
        let _ = self.stream.stop_capture();
    }
}

impl SourceStream for NativeStream {
    fn detach_output(&self) -> Result<(), String> {
        self.stream
            .remove_recording_output(&self.output)
            .map_err(|error| error.to_string())
    }

    fn remove_handlers(&mut self) -> Result<(), String> {
        let mut result = Ok(());
        for (id, kind) in std::mem::take(&mut self.handlers) {
            match self.stream.remove_output_handler(id, kind) {
                Ok(true) => {}
                Ok(false) => {
                    result = Err(format!("the {kind:?} handler was already gone"));
                }
                Err(error) => result = Err(error.to_string()),
            }
        }
        result
    }

    fn stopper(&self) -> Stopper {
        let stream = self.stream.clone();
        Box::new(move || stream.stop_capture().map_err(|error| error.to_string()))
    }

    fn recorded_duration(&self) -> Option<MediaTime> {
        let duration = self.output.recorded_duration();
        if !duration.is_valid() {
            return None;
        }
        MediaTime::new(duration.value, duration.timescale)
    }

    fn exclude(&self, exclude: &Exclusions) -> Result<(), Error> {
        let Some(id) = filtered_display(self.source) else {
            return Ok(());
        };
        let content = SCShareableContent::get()
            .map_err(|error| Error::ShareableContent(error.to_string()))?;
        let display = find_display(&content, id).ok_or(Error::SourceUnavailable(self.source))?;
        let filter = display_filter(&content, &display, exclude)?;
        // The region's area is the configuration's `sourceRect`, which a new filter leaves alone.
        self.stream
            .update_content_filter(&filter)
            .map_err(|error| Error::Exclusion(error.to_string()))
    }
}

/// The display whose filter carries a source's exclusions. A window's filter captures only that
/// window, so it has none.
fn filtered_display(source: Source) -> Option<u32> {
    match source {
        Source::Display(id) | Source::Region { display: id, .. } => Some(id),
        Source::Window(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_change_the_display_filter_of_displays_and_regions_only() {
        let rect = Rect {
            x: 10.0,
            y: 20.0,
            width: 300.0,
            height: 200.0,
        };
        assert_eq!(filtered_display(Source::Display(3)), Some(3));
        assert_eq!(
            filtered_display(Source::Region { display: 4, rect }),
            Some(4)
        );
        assert_eq!(filtered_display(Source::Window(7)), None);
    }
}
