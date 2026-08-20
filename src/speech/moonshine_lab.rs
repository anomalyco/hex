use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use color_eyre::eyre::{Result, WrapErr, bail};
use crossterm::event::{self, Event as TerminalEvent, KeyCode, KeyEventKind};
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::audio::{AudioInput, AudioInputEvent};
use crate::config::INPUT_DEVICES;
use crate::events::TranscriptPhase;
use crate::moonshine::{Moonshine, MoonshineConfig, RecognitionUpdate};
use crate::spoken_text::normalize;

const MANIFEST_NAME: &str = "manifest.json";

#[derive(Deserialize, Serialize)]
struct Manifest {
    clips: Vec<Clip>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Clip {
    id: String,
    audio: Option<PathBuf>,
    expected: String,
    #[serde(default)]
    instruction: String,
}

#[derive(Clone, Copy)]
struct Profile {
    name: &'static str,
    config: MoonshineConfig,
}

const PROFILES: &[Profile] = &[
    Profile {
        name: "CPU 500ms",
        config: MoonshineConfig {
            transcription_interval_ms: 500,
            word_timestamps: false,
        },
    },
    Profile {
        name: "CPU 300ms",
        config: MoonshineConfig {
            transcription_interval_ms: 300,
            word_timestamps: false,
        },
    },
    Profile {
        name: "CPU 200ms",
        config: MoonshineConfig {
            transcription_interval_ms: 200,
            word_timestamps: false,
        },
    },
    Profile {
        name: "CPU 200ms + words",
        config: MoonshineConfig {
            transcription_interval_ms: 200,
            word_timestamps: true,
        },
    },
];

enum Mode {
    Browse,
    Recording { samples: Vec<f32>, started: Instant },
    Editing { original: String, value: String },
    ConfirmDelete,
}

struct BenchmarkResult {
    profile: &'static str,
    transcript: String,
    expected: String,
    load_ms: u128,
    inference_ms: u128,
    updates: Vec<RecognitionUpdate>,
}

pub fn run(project_root: &Path, directory: &Path, device: Option<&str>) -> Result<()> {
    fs::create_dir_all(directory.join("audio"))?;
    let manifest_path = directory.join(MANIFEST_NAME);
    let manifest = load_manifest(&manifest_path)?;
    save_manifest(&manifest_path, &manifest)?;
    let input = match device {
        Some(device) => AudioInput::open_named(device)?,
        None => AudioInput::open(INPUT_DEVICES)?,
    };
    let mut terminal = ratatui::init();
    let result = run_loop(
        &mut terminal,
        project_root,
        directory,
        manifest_path,
        manifest,
        input,
    );
    ratatui::restore();
    result.map_err(Into::into)
}

pub fn run_batch(project_root: &Path, directory: &Path) -> Result<()> {
    let manifest = load_manifest(&directory.join(MANIFEST_NAME))?;
    let recorded = manifest
        .clips
        .iter()
        .filter_map(|clip| {
            clip.audio
                .as_ref()
                .map(|audio| (clip, directory.join(audio)))
        })
        .collect::<Vec<_>>();
    if recorded.is_empty() {
        bail!("the Moonshine corpus has no recorded fixtures");
    }
    let mut output = io::BufWriter::new(io::stdout().lock());
    for profile in PROFILES {
        eprintln!("loading profile: {}", profile.name);
        let load_started = Instant::now();
        let mut recognizer = Moonshine::load_with_config(project_root, 1, profile.config)?;
        let load_ms = load_started.elapsed().as_millis();
        let mut matches = 0_usize;
        let mut inference_ms = 0_u128;
        for (clip, audio) in &recorded {
            eprintln!("running fixture: {} / {}", profile.name, clip.id);
            let result =
                benchmark_clip_with_recognizer(audio, clip, *profile, &mut recognizer, load_ms)?;
            let matched = normalize(&result.transcript) == normalize(&result.expected);
            matches += usize::from(matched);
            inference_ms += result.inference_ms;
            writeln!(
                output,
                "{}",
                json!({
                    "type": "clip",
                    "profile": profile.name,
                    "id": clip.id,
                    "matched": matched,
                    "expected": result.expected,
                    "transcript": result.transcript,
                    "load_ms": result.load_ms,
                    "inference_ms": result.inference_ms,
                    "updates": result.updates.len(),
                })
            )?;
            output.flush()?;
        }
        writeln!(
            output,
            "{}",
            json!({
                "type": "summary",
                "profile": profile.name,
                "matched": matches,
                "clips": recorded.len(),
                "load_ms": load_ms,
                "inference_ms": inference_ms,
            })
        )?;
        output.flush()?;
    }
    for clip in manifest.clips.iter().filter(|clip| clip.audio.is_none()) {
        eprintln!("pending fixture: {} ({})", clip.id, clip.expected);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_loop(
    terminal: &mut ratatui::DefaultTerminal,
    project_root: &Path,
    directory: &Path,
    manifest_path: PathBuf,
    mut manifest: Manifest,
    input: AudioInput,
) -> io::Result<()> {
    let mut selected = manifest
        .clips
        .iter()
        .position(|clip| clip.audio.is_none())
        .unwrap_or(0);
    let mut profile = 0_usize;
    let mut mode = Mode::Browse;
    let mut result = None::<BenchmarkResult>;
    let mut benchmark = None::<Receiver<std::result::Result<BenchmarkResult, String>>>;
    let mut status = format!("microphone: {}", input.device_name);

    loop {
        let mut clear_terminal = false;
        while let AudioInputEvent::Chunk { samples, .. } = input.recv_timeout(Duration::ZERO) {
            if let Mode::Recording {
                samples: recording, ..
            } = &mut mode
            {
                recording.extend(samples);
            }
        }
        if let Some(receiver) = &benchmark {
            match receiver.try_recv() {
                Ok(Ok(completed)) => {
                    status = format!("{} completed", completed.profile);
                    result = Some(completed);
                    benchmark = None;
                    clear_terminal = true;
                }
                Ok(Err(error)) => {
                    status = error;
                    benchmark = None;
                    clear_terminal = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    status = "benchmark worker disconnected".into();
                    benchmark = None;
                    clear_terminal = true;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        selected = selected.min(manifest.clips.len().saturating_sub(1));
        if clear_terminal {
            terminal.clear()?;
        }
        terminal.draw(|frame| {
            draw(
                frame,
                &manifest,
                selected,
                PROFILES[profile],
                &mode,
                result.as_ref(),
                benchmark.is_some(),
                &status,
            )
        })?;

        if !event::poll(Duration::from_millis(20))? {
            continue;
        }
        let TerminalEvent::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match &mut mode {
            Mode::Editing { original, value } => match key.code {
                KeyCode::Esc => {
                    if let Some(clip) = manifest.clips.get_mut(selected) {
                        clip.expected = original.clone();
                    }
                    mode = Mode::Browse;
                }
                KeyCode::Enter => {
                    if let Some(clip) = manifest.clips.get_mut(selected) {
                        clip.expected = value.trim().to_string();
                    }
                    if let Err(error) = save_manifest(&manifest_path, &manifest) {
                        status = error.to_string();
                    }
                    mode = Mode::Browse;
                }
                KeyCode::Backspace => {
                    value.pop();
                }
                KeyCode::Char(character) => value.push(character),
                _ => {}
            },
            Mode::Recording { .. } => {
                if matches!(key.code, KeyCode::Char(' ') | KeyCode::Enter) {
                    let recording = std::mem::replace(&mut mode, Mode::Browse);
                    let Mode::Recording { samples, started } = recording else {
                        unreachable!()
                    };
                    match save_recording(
                        directory,
                        input.sample_rate,
                        samples,
                        selected,
                        &mut manifest,
                        &manifest_path,
                    ) {
                        Ok(()) => {
                            status = format!("recorded {:.1}s", started.elapsed().as_secs_f32());
                            if let Some(next) = manifest
                                .clips
                                .iter()
                                .enumerate()
                                .skip(selected + 1)
                                .find_map(|(index, clip)| clip.audio.is_none().then_some(index))
                                .or_else(|| {
                                    manifest.clips.iter().position(|clip| clip.audio.is_none())
                                })
                            {
                                selected = next;
                            }
                        }
                        Err(error) => status = error.to_string(),
                    }
                } else if key.code == KeyCode::Esc {
                    mode = Mode::Browse;
                    status = "recording discarded".into();
                }
            }
            Mode::ConfirmDelete => match key.code {
                KeyCode::Char('d') if !manifest.clips.is_empty() => {
                    let clip = manifest.clips.remove(selected);
                    if let Some(audio) = clip.audio {
                        let _ = fs::remove_file(directory.join(audio));
                    }
                    if let Err(error) = save_manifest(&manifest_path, &manifest) {
                        status = error.to_string();
                    } else {
                        status = format!("deleted {}", clip.id);
                    }
                    mode = Mode::Browse;
                }
                _ => mode = Mode::Browse,
            },
            Mode::Browse => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = (selected + 1).min(manifest.clips.len().saturating_sub(1));
                    result = None;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    selected = selected.saturating_sub(1);
                    result = None;
                }
                KeyCode::Char(' ') if !manifest.clips.is_empty() => {
                    mode = Mode::Recording {
                        samples: Vec::new(),
                        started: Instant::now(),
                    };
                    result = None;
                    status = "recording".into();
                }
                KeyCode::Char('e') if !manifest.clips.is_empty() => {
                    let value = manifest.clips[selected].expected.clone();
                    mode = Mode::Editing {
                        original: value.clone(),
                        value,
                    };
                }
                KeyCode::Char('d') if !manifest.clips.is_empty() => {
                    mode = Mode::ConfirmDelete;
                }
                KeyCode::Char('p') => {
                    profile = (profile + 1) % PROFILES.len();
                    result = None;
                    status = PROFILES[profile].name.into();
                }
                KeyCode::Char('r')
                    if benchmark.is_none()
                        && manifest
                            .clips
                            .get(selected)
                            .is_some_and(|clip| clip.audio.is_some()) =>
                {
                    let clip = manifest.clips[selected].clone();
                    let audio = directory.join(
                        clip.audio
                            .as_ref()
                            .expect("recorded fixture has an audio path"),
                    );
                    let root = project_root.to_path_buf();
                    let selected_profile = PROFILES[profile];
                    let (sender, receiver) = mpsc::sync_channel(1);
                    thread::spawn(move || {
                        let completed = benchmark_clip(&root, &audio, &clip, selected_profile)
                            .map_err(|error| error.to_string());
                        let _ = sender.send(completed);
                    });
                    benchmark = Some(receiver);
                    status = format!("running {}", selected_profile.name);
                }
                _ => {}
            },
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw(
    frame: &mut ratatui::Frame,
    manifest: &Manifest,
    selected: usize,
    profile: Profile,
    mode: &Mode,
    result: Option<&BenchmarkResult>,
    running: bool,
    status: &str,
) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Percentage(38),
        Constraint::Min(8),
        Constraint::Length(2),
    ])
    .split(frame.area());
    let title = match mode {
        Mode::Recording { started, samples } => format!(
            " RECORDING {:.1}s · {} samples ",
            started.elapsed().as_secs_f32(),
            samples.len()
        ),
        Mode::Editing { .. } => " EDIT EXPECTED TRANSCRIPT ".into(),
        Mode::ConfirmDelete => " PRESS d AGAIN TO DELETE ".into(),
        Mode::Browse if running => " MOONSHINE LAB · RUNNING ".into(),
        Mode::Browse => " MOONSHINE LAB ".into(),
    };
    let recorded = manifest
        .clips
        .iter()
        .filter(|clip| clip.audio.is_some())
        .count();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                title,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!(
                "  profile: {} · recorded: {recorded}/{}",
                profile.name,
                manifest.clips.len()
            )),
        ]))
        .block(Block::default().borders(Borders::ALL)),
        areas[0],
    );

    let clips = if manifest.clips.is_empty() {
        vec![Line::styled(
            "No fixtures. Press Space to record one.",
            Style::default().fg(Color::DarkGray),
        )]
    } else {
        manifest
            .clips
            .iter()
            .enumerate()
            .map(|(index, clip)| {
                let marker = if index == selected { "›" } else { " " };
                let state = if clip.audio.is_some() { "●" } else { "○" };
                let style = if index == selected {
                    Style::default().fg(Color::Black).bg(Color::Cyan)
                } else {
                    Style::default()
                };
                Line::styled(
                    format!("{marker} {state} {:<24} {}", clip.id, clip.expected),
                    style,
                )
            })
            .collect()
    };
    let visible_clips = areas[1].height.saturating_sub(2) as usize;
    let clip_scroll = selected.saturating_sub(visible_clips.saturating_sub(1));
    frame.render_widget(
        Paragraph::new(clips)
            .block(Block::default().title(" Fixtures ").borders(Borders::ALL))
            .scroll((clip_scroll as u16, 0))
            .wrap(Wrap { trim: false }),
        areas[1],
    );

    let details = match mode {
        Mode::Editing { value, .. } => vec![
            Line::raw("Type the expected transcript or command phrase:"),
            Line::styled(value, Style::default().fg(Color::Yellow)),
            Line::raw(""),
            Line::styled(
                "Enter save · Esc cancel",
                Style::default().fg(Color::DarkGray),
            ),
        ],
        _ => result.map_or_else(
            || {
                let mut lines = manifest.clips.get(selected).map_or_else(Vec::new, |clip| {
                    vec![
                        Line::from(vec![
                            Span::styled("SAY: ", Style::default().fg(Color::Cyan)),
                            Span::styled(
                                &clip.expected,
                                Style::default()
                                    .fg(Color::Yellow)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]),
                        Line::raw(""),
                        Line::raw(&clip.instruction),
                        Line::raw(""),
                    ]
                });
                lines.push(Line::styled(status, Style::default().fg(Color::DarkGray)));
                lines
            },
            |result| {
                let matches = normalize(&result.transcript) == normalize(&result.expected)
                    && !normalize(&result.expected).is_empty();
                let mut lines = vec![
                    Line::from(vec![
                        Span::styled(
                            if matches { "MATCH" } else { "DIFF" },
                            Style::default().fg(if matches { Color::Green } else { Color::Yellow }),
                        ),
                        Span::raw(format!(
                            " · {} · load {}ms · inference {}ms · {} updates",
                            result.profile,
                            result.load_ms,
                            result.inference_ms,
                            result.updates.len()
                        )),
                    ]),
                    Line::raw(format!("expected:   {}", result.expected)),
                    Line::raw(format!("transcript: {}", result.transcript)),
                    Line::raw(""),
                ];
                lines.extend(result.updates.iter().rev().take(8).rev().map(|update| {
                    let words = update
                        .words
                        .iter()
                        .map(|word| {
                            format!(
                                "{}@{}-{}:{:.2}",
                                word.text, word.start_ms, word.end_ms, word.confidence
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    Line::raw(format!(
                        "{:?} {}-{}ms {}ms · {}{}",
                        update.phase,
                        update.start_ms,
                        update.end_ms,
                        update.latency_ms,
                        update.text,
                        if words.is_empty() {
                            String::new()
                        } else {
                            format!(" · {words}")
                        }
                    ))
                }));
                lines
            },
        ),
    };
    frame.render_widget(
        Paragraph::new(details)
            .block(Block::default().title(" Result ").borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        areas[2],
    );
    frame.render_widget(
        Paragraph::new(
            "Space record/stop · ↑↓ select · e edit · r run · p profile · d d delete · q quit",
        )
        .style(Style::default().fg(Color::DarkGray)),
        areas[3],
    );
}

fn load_manifest(path: &Path) -> Result<Manifest> {
    if !path.exists() {
        return Ok(default_manifest());
    }
    let bytes = fs::read(path)?;
    let mut manifest = serde_json::from_slice::<Manifest>(&bytes)
        .wrap_err_with(|| format!("could not parse {}", path.display()))?;
    for clip in default_manifest().clips {
        if !manifest.clips.iter().any(|existing| existing.id == clip.id) {
            manifest.clips.push(clip);
        }
    }
    Ok(manifest)
}

fn default_manifest() -> Manifest {
    let clips = [
        (
            "go-back",
            "go back",
            "Speak at your normal command volume, then leave about one second of silence.",
        ),
        (
            "go-next",
            "go next",
            "Speak at your normal command volume, then leave about one second of silence.",
        ),
        (
            "repeat-chain",
            "go back go back",
            "Say both commands in one smooth utterance without an intentional pause.",
        ),
        (
            "mixed-chain",
            "go back go next",
            "Say both commands in one smooth utterance without an intentional pause.",
        ),
        (
            "paused-chain",
            "go back go back",
            "Use the short natural pause you would normally make between chained commands.",
        ),
        (
            "quiet-command",
            "go back",
            "Speak more quietly than usual while remaining intelligible.",
        ),
        (
            "command-number",
            "command five",
            "Speak naturally; this exercises the typed digit capture.",
        ),
        (
            "movement-count",
            "go down five",
            "Speak naturally; this exercises direction and count captures.",
        ),
        (
            "selection-count",
            "select right three",
            "Speak naturally; this exercises modified repeated movement.",
        ),
        (
            "dictation-start",
            "dictate",
            "Record the standalone voice-dictation start phrase.",
        ),
        (
            "dictation-paste",
            "say paste",
            "Record the standalone control as you would say it after dictating.",
        ),
        (
            "dictation-send",
            "say send",
            "Record the standalone control as you would say it after dictating.",
        ),
        (
            "dictation-cancel",
            "never mind",
            "Record the standalone cancellation control.",
        ),
        (
            "paste-suffix",
            "this is a test message say paste",
            "Say the message and control continuously, with your normal boundary before the control.",
        ),
        (
            "send-suffix",
            "this is a test message say send",
            "Say the message and control continuously, with your normal boundary before the control.",
        ),
        (
            "triple-chain",
            "go back go next go back",
            "Say all three commands smoothly as one utterance.",
        ),
        (
            "five-command-chain",
            "go back go next go back go next go back",
            "Say the five commands continuously at a comfortable pace.",
        ),
        (
            "mixed-capture-chain",
            "go down three select right two command five go back",
            "Chain movement, selection, a digit shortcut, and navigation without a long pause.",
        ),
        (
            "count-chain",
            "go left ten go down five select up three",
            "Say each direction and count clearly in one chained utterance.",
        ),
        (
            "long-paused-chain",
            "go back go down three select right two go next",
            "Use short natural pauses between commands, but do not stop the recording.",
        ),
        (
            "digit-chain",
            "command two command five command nine",
            "Say three numbered shortcuts in one smooth chain.",
        ),
        (
            "key-home",
            "key home",
            "Speak naturally; this contrasts a short key name with other two-word commands.",
        ),
        (
            "key-end",
            "key end",
            "Speak naturally; this is the minimal pair for key home.",
        ),
        (
            "open-zed",
            "open zed",
            "Say the application command at your normal command volume.",
        ),
        (
            "open-ghostty",
            "open ghostty",
            "Say the application name naturally without over-enunciating it.",
        ),
        (
            "application-chain",
            "open zed go down three select right two",
            "Chain an application command with two keyboard commands.",
        ),
        (
            "current-dictation-stop",
            "dictate stop",
            "Record the current voice-dictation stop control as a standalone phrase.",
        ),
        (
            "current-dictation-send",
            "dictate send",
            "Record the current voice-dictation send control as a standalone phrase.",
        ),
        (
            "current-dictation-cancel",
            "dictate cancel",
            "Record the current voice-dictation cancellation control as a standalone phrase.",
        ),
        (
            "current-send-suffix",
            "please update the release notes before lunch dictate send",
            "Dictate the message naturally, then append the current send control without restarting.",
        ),
        (
            "current-stop-suffix",
            "the command should preserve punctuation and capitalization dictate stop",
            "Dictate the message naturally, then append the current stop control without restarting.",
        ),
        (
            "fast-mixed-chain",
            "go back command five go down three go next",
            "Say the complete chain faster than usual while keeping it intelligible.",
        ),
        (
            "quiet-long-chain",
            "go left three select down two go back",
            "Say the complete chain more quietly than usual while remaining intelligible.",
        ),
        (
            "near-miss-natural-sentence",
            "I should go back to the next section before lunch",
            "Speak as ordinary prose, not as a command; this should not form a complete command sequence.",
        ),
        (
            "near-miss-polite-command",
            "please go back when you are ready",
            "Speak as an ordinary request; the extra words should prevent full command decomposition.",
        ),
    ]
    .into_iter()
    .map(|(id, expected, instruction)| Clip {
        id: id.into(),
        audio: None,
        expected: expected.into(),
        instruction: instruction.into(),
    })
    .collect();
    Manifest { clips }
}

fn save_manifest(path: &Path, manifest: &Manifest) -> Result<()> {
    let temporary = path.with_extension("json.tmp");
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)?;
    Ok(())
}

fn save_recording(
    directory: &Path,
    sample_rate: u32,
    samples: Vec<f32>,
    selected: usize,
    manifest: &mut Manifest,
    manifest_path: &Path,
) -> Result<()> {
    if samples.len() < sample_rate as usize / 4 {
        bail!("recording is shorter than 250ms");
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let clip = manifest
        .clips
        .get_mut(selected)
        .ok_or_else(|| color_eyre::eyre::eyre!("no fixture is selected"))?;
    let relative = PathBuf::from(format!("audio/{}-{timestamp}.wav", clip.id));
    let path = directory.join(&relative);
    let mut writer = WavWriter::create(
        &path,
        WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 32,
            sample_format: SampleFormat::Float,
        },
    )?;
    for sample in samples {
        writer.write_sample(sample)?;
    }
    writer.finalize()?;
    let previous = clip.audio.replace(relative);
    save_manifest(manifest_path, manifest)?;
    if let Some(previous) = previous {
        let _ = fs::remove_file(directory.join(previous));
    }
    Ok(())
}

fn benchmark_clip(
    project_root: &Path,
    path: &Path,
    clip: &Clip,
    profile: Profile,
) -> Result<BenchmarkResult> {
    let load_started = Instant::now();
    let mut recognizer = Moonshine::load_with_config(project_root, 1, profile.config)?;
    let load_ms = load_started.elapsed().as_millis();
    benchmark_clip_with_recognizer(path, clip, profile, &mut recognizer, load_ms)
}

fn benchmark_clip_with_recognizer(
    path: &Path,
    clip: &Clip,
    profile: Profile,
    recognizer: &mut Moonshine,
    load_ms: u128,
) -> Result<BenchmarkResult> {
    let mut reader = WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.sample_format != SampleFormat::Float || spec.bits_per_sample != 32
    {
        bail!("Moonshine Lab recordings must be mono 32-bit float WAV files");
    }
    let samples = reader.samples::<f32>().collect::<Result<Vec<_>, _>>()?;
    let chunk_size = (spec.sample_rate as usize / 50).max(1);
    let mut updates = Vec::new();
    let mut inference_ms = 0_u128;
    for (index, chunk) in samples.chunks(chunk_size).enumerate() {
        recognizer.add_audio(chunk, spec.sample_rate)?;
        if index % 10 == 9 {
            let started = Instant::now();
            updates.extend(recognizer.update()?);
            inference_ms += started.elapsed().as_millis();
        }
    }
    let started = Instant::now();
    updates.extend(recognizer.finish_stream(0)?);
    inference_ms += started.elapsed().as_millis();
    recognizer.reset_stream()?;

    let mut lines = HashMap::new();
    for update in &updates {
        let line =
            lines
                .entry(update.line_id)
                .or_insert((update.start_ms, String::new(), update.phase));
        line.0 = update.start_ms;
        line.2 = update.phase;
        if !update.text.trim().is_empty() {
            line.1.clone_from(&update.text);
        }
    }
    let mut lines = lines.into_values().collect::<Vec<_>>();
    lines.sort_by_key(|(start, _, _)| *start);
    let transcript = lines
        .into_iter()
        .filter(|(_, text, phase)| !text.trim().is_empty() && *phase == TranscriptPhase::Completed)
        .map(|(_, text, _)| text)
        .collect::<Vec<_>>()
        .join(" ");
    Ok(BenchmarkResult {
        profile: profile.name,
        transcript,
        expected: clip.expected.clone(),
        load_ms,
        inference_ms,
        updates,
    })
}
