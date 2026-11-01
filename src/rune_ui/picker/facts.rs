//! What the speech-model picker says about each model, in plain measures
//! both of its views share, and the pieces that show a model's one state.
//!
//! - Wait when you stop: seconds between letting go and the text after 30
//!   seconds of talking, from the upstream speed figure; none for a model
//!   that writes while you talk.
//! - Wrong words: "1 in 62", from the error rate, only when the model was
//!   measured in the saved language.
//! - Languages, the download, and whether it reads your vocabulary.

use rune::{El, TextStyle, row, text};
use rune_kit::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Progress};

use super::{self as picker, PickerModel, cancel, choose};
use crate::desktop::i18n::{tr, tr_fill};
use crate::rune_ui::Ui;
use crate::rune_ui::controls::{ButtonKind, Paint, PushSize, button, status_dot};
use crate::transcription::model_benchmarks::ErrorMetric;
use crate::transcription::models::{
    self as transcription_models, AUTO_LANGUAGE, ModelDefinition, ModelRuntime, Recommendation,
    TranscriptionModelId, VocabularyUse, language_name,
};

/// The one state a model shows.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum State {
    InUse,
    Downloaded,
    /// Another app downloaded this exact file; Use checks it in place.
    OnComputer,
    NotDownloaded,
    Preparing {
        label: String,
        fraction: Option<f32>,
    },
}

impl State {
    /// Whether choosing it asks the service for it.
    pub(super) fn choosable(&self) -> bool {
        matches!(
            self,
            Self::Downloaded | Self::OnComputer | Self::NotDownloaded
        )
    }
}

pub(super) struct Entry {
    pub(super) def: &'static ModelDefinition,
    pub(super) recommendation: Option<Recommendation>,
    pub(super) supports: bool,
    pub(super) state: State,
}

impl Entry {
    pub(super) fn in_use(&self) -> bool {
        self.state == State::InUse
    }

    pub(super) fn preparing(&self) -> bool {
        matches!(self.state, State::Preparing { .. })
    }
}

pub(super) fn entries(ui: &Ui, language: &str, models: &[PickerModel]) -> Vec<Entry> {
    let details = picker::benchmark_details_on(ui);
    models
        .iter()
        .map(|&(def, recommendation)| Entry {
            def,
            recommendation: picker::shown_recommendation(recommendation, def, language, details),
            supports: def.supports_language(language),
            state: state(ui, def, language),
        })
        .collect()
}

fn state(ui: &Ui, def: &'static ModelDefinition, language: &str) -> State {
    let installed = ui.picker.installed(def, def.language_for(language));
    if let Some((label, fraction)) = picker::preparation(ui, def) {
        State::Preparing { label, fraction }
    } else if picker::transcription_selection_is_active(&ui.settings.transcription, def, installed)
    {
        State::InUse
    } else if installed {
        State::Downloaded
    } else if !def.id.is_custom()
        && ui
            .found
            .models
            .iter()
            .any(|found| found.catalog == Some(def.id))
    {
        State::OnComputer
    } else {
        State::NotDownloaded
    }
}

const TALK_SECONDS: f32 = 30.0;

/// Seconds between letting go and the text after 30 seconds of talking:
/// zero for a model that writes while you talk, `None` without a figure.
pub(super) fn wait(def: &ModelDefinition) -> Option<f32> {
    if def.streams {
        return Some(0.0);
    }
    let factor: f32 = def
        .realtime
        .trim_start_matches('~')
        .strip_suffix('x')?
        .parse()
        .ok()?;
    (factor > 0.0).then(|| TALK_SECONDS / factor)
}

/// The wait in whole seconds as people feel it: none, under a second, or
/// seconds. The upstream figures carry no finer precision.
fn wait_seconds(def: &ModelDefinition) -> Option<u32> {
    wait(def).map(|seconds| {
        if seconds == 0.0 {
            0
        } else {
            seconds.round().max(1.0) as u32
        }
    })
}

pub(super) fn wait_label(def: &ModelDefinition) -> String {
    match (wait(def), wait_seconds(def)) {
        (None, _) | (_, None) => "—".into(),
        (Some(0.0), _) => tr("No wait").into(),
        (Some(seconds), _) if seconds < 1.0 => tr("Under 1 s").into(),
        (_, Some(seconds)) => tr_fill("{} s", &seconds.to_string()),
    }
}

/// Higher is better: a shorter wait as people feel it.
pub(super) fn wait_score(def: &ModelDefinition) -> Option<f64> {
    let seconds = wait(def)?;
    Some(if seconds == 0.0 {
        1.0
    } else if seconds < 1.0 {
        0.0
    } else {
        -f64::from(seconds.round())
    })
}

/// Words (or characters) per wrong one, when `def` was measured in
/// `language`.
pub(super) fn errors(def: &ModelDefinition, language: &str) -> Option<(ErrorMetric, f64)> {
    let record = transcription_models::benchmark(def)?;
    (record.language == language && record.value > 0.0)
        .then(|| (record.metric, (100.0 / record.value).round()))
}

pub(super) fn errors_label(per: f64) -> String {
    tr_fill("1 in {}", &format!("{per:.0}"))
}

pub(super) fn errors_heading(metric: ErrorMetric) -> &'static str {
    match metric {
        ErrorMetric::Wer => tr("Wrong words"),
        ErrorMetric::Cer => tr("Wrong characters"),
    }
}

/// The languages Hex offers with `def`.
pub(super) fn language_count(def: &ModelDefinition) -> usize {
    def.language_choices()
        .iter()
        .filter(|code| **code != AUTO_LANGUAGE)
        .count()
}

/// "English" for a one-language model, else how many.
pub(super) fn languages_label(def: &ModelDefinition) -> String {
    match def.language_choices().as_slice() {
        [only] | [only, AUTO_LANGUAGE] => language_name(only).to_owned(),
        _ => language_count(def).to_string(),
    }
}

/// Whether transcription reads the vocabulary: Whisper and Qwen3-ASR as
/// their prompt, Parakeet only with Labs › Vocabulary biasing.
pub(super) fn reads_vocabulary(ui: &Ui, def: &ModelDefinition) -> bool {
    match def.vocabulary_use() {
        VocabularyUse::Prompt => true,
        VocabularyUse::Bias => {
            let feature = crate::labs::Feature::VocabularyBiasing;
            feature.shipped() && feature.on(&ui.settings)
        }
        VocabularyUse::None => false,
    }
}

/// Bytes a choice still has to download.
pub(super) fn to_download(entry: &Entry) -> u64 {
    match entry.state {
        State::NotDownloaded | State::Preparing { .. } => entry.def.download_bytes().unwrap_or(0),
        _ => 0,
    }
}

/// The download's size, or who keeps the model when Hex downloads none.
pub(super) fn size_text(def: &ModelDefinition) -> String {
    match def.runtime {
        ModelRuntime::AppleSpeech => tr("Managed by macOS").into(),
        ModelRuntime::Gguf(_) => def.size_label(),
    }
}

/// The download row's value: the size, or that nothing is left to fetch.
pub(super) fn download_label(entry: &Entry) -> String {
    match entry.state {
        State::InUse | State::Downloaded => tr("Downloaded").into(),
        State::OnComputer => tr("On this computer").into(),
        _ => size_text(entry.def),
    }
}

/// Whether choosing it starts a download.
fn downloads(entry: &Entry) -> bool {
    entry.state == State::NotDownloaded && entry.def.download_bytes().is_some()
}

/// One plain line on why you'd pick it.
pub(super) fn blurb(def: &ModelDefinition) -> &'static str {
    use TranscriptionModelId as M;
    match def.id {
        M::ParakeetUnifiedEnglish => {
            tr("Writes while you talk, so the text is ready when you let go")
        }
        M::CohereTranscribe => tr("The fewest wrong words, in 14 languages. A big download"),
        M::WhisperLargeV3Turbo => tr("Almost any language, and it reads your vocabulary"),
        M::ParakeetV2 => tr("The English model before Parakeet Unified"),
        M::ParakeetV3 => tr("25 European languages, and it hears which one you speak"),
        M::ParakeetUltra => tr("Parakeet v3, retrained for fewer wrong words"),
        M::Qwen3Asr06B => tr("Chinese and 29 more, and it reads your vocabulary"),
        M::QwenCleoAsr17B => tr("Egyptian Arabic, mixed with English"),
        M::IvritWhisperLargeV3Turbo => tr("Whisper, tuned for Hebrew"),
        M::SenseVoiceSmall => tr("Chinese, Japanese, Korean and English. A small download"),
        M::UnderdogBark3Pro => tr("Parakeet, tuned for fewer wrong words in English"),
        M::UnderdogBark3Clean => tr("English, with the ums and uhs taken out"),
        M::Nemotron35Asr => tr("Writes while you talk, in 27 languages"),
        M::AppleSpeech => tr("Apple's speech recognition, kept up to date by macOS"),
        M::Custom(_) => "",
    }
}

/// A list row's recommendation, and whether it is the one to read first.
pub(super) fn list_role(entry: &Entry) -> Option<(&'static str, bool)> {
    Some(match entry.recommendation? {
        Recommendation::Recommended => (tr("Recommended"), true),
        Recommendation::MostAccurate => (tr("Fewest wrong words"), false),
        Recommendation::Fastest => (tr("Fastest"), false),
        Recommendation::Vocabulary => (tr("Reads your vocabulary"), false),
    })
}

/// What a model is best at among `others`, for its eyebrow.
pub(super) fn role(ui: &Ui, entry: &Entry, others: &[&Entry]) -> Option<&'static str> {
    match entry.recommendation {
        Some(Recommendation::Recommended) => Some(tr("Recommended")),
        Some(Recommendation::MostAccurate) => Some(tr("Fewest wrong words")),
        Some(Recommendation::Fastest) => Some(tr("Fastest")),
        Some(Recommendation::Vocabulary) => Some(tr("Reads your vocabulary")),
        None if others.len() > 1
            && others.iter().all(|other| {
                language_count(other.def) < language_count(entry.def)
                    || other.def.id == entry.def.id
            }) =>
        {
            Some(tr("Most languages"))
        }
        None if reads_vocabulary(ui, entry.def) => Some(tr("Reads your vocabulary")),
        None => None,
    }
}

/// The picker's title for the saved language.
pub(super) fn title(language: &str) -> String {
    if language == AUTO_LANGUAGE {
        tr("Speech model that detects the language").into()
    } else {
        crate::desktop::i18n::tr_named(
            "Speech model for {language}",
            &[("language", language_name(language))],
        )
    }
}

pub(super) fn tnum(style: TextStyle) -> TextStyle {
    style.features("'tnum' 1")
}

pub(super) fn in_use(p: Paint) -> El {
    row()
        .key("in-use")
        .h(28.0)
        .gap(8.0)
        .center_y()
        .child(status_dot(p.accent()))
        .child(
            text(tr("In use"))
                .key("label")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
}

/// The × that stops a download or preparation.
pub(super) fn cancel_button(p: Paint) -> El {
    IconButton::new(p.k.glyph.close)
        .variant(IconButtonVariant::Ghost)
        .control_size(ControlSize::Small)
        .label(tr("Cancel"))
        .on_press(cancel)
        .build(p.k)
        .key("cancel-model")
}

/// Progress that fits a narrow cell: the percentage and size (or the
/// stage) over a bar, with the × at the end.
pub(super) fn compact_progress(p: Paint, entry: &Entry) -> El {
    let State::Preparing { label, fraction } = &entry.state else {
        return row();
    };
    let mut readout = row().key("readout").gap(4.0);
    match fraction {
        Some(fraction) => {
            readout = readout
                .child(
                    text(format!("{:.0}%", fraction * 100.0))
                        .key("percent")
                        .font(tnum(p.k.ty.small_medium))
                        .color(p.text()),
                )
                .child(
                    text(tr_fill("of {}", &entry.def.size_label()))
                        .key("of")
                        .font(tnum(p.k.ty.small))
                        .color(p.faint()),
                );
        }
        None => {
            readout = readout.child(
                text(label.clone())
                    .key("stage")
                    .font(tnum(p.k.ty.small_medium))
                    .color(p.text())
                    .truncate(),
            );
        }
    }
    row()
        .key("preparing")
        .w_full()
        .gap(8.0)
        .center_y()
        .child(
            rune::col()
                .key("progress")
                .grow(1.0)
                .shrink()
                .gap(6.0)
                .child(readout)
                .child(
                    Progress::new(*fraction)
                        .build(p.k)
                        .w_full()
                        .label(label.clone()),
                ),
        )
        .child(cancel_button(p))
}

/// Use or Download for a model that can be chosen, at the button size of
/// the compare columns.
pub(super) fn choose_button(p: Paint, entry: &Entry, primary: bool) -> El {
    let def = entry.def;
    let label = if downloads(entry) {
        tr_fill("Download {}", &def.size_label())
    } else {
        tr("Use").into()
    };
    button(
        p,
        "choose-model",
        &label,
        if primary {
            ButtonKind::Primary
        } else {
            ButtonKind::Secondary
        },
        move |ui: &mut Ui, _| choose(ui, def),
    )
}

/// The list's quieter Use or download action, at row size.
pub(super) fn row_choose_button(p: Paint, entry: &Entry) -> El {
    let def = entry.def;
    if !downloads(entry) {
        return Button::new(tr("Use"))
            .variant(ButtonVariant::Neutral)
            .on_press_tx(move |ui: &mut Ui, _| choose(ui, def))
            .build_small_push(p.k)
            .key("choose-model");
    }
    let size = def.size_label();
    Button::new(tr_fill("Download {}", &size))
        .variant(ButtonVariant::Ghost)
        .label_with(move |font, ink| {
            row()
                .gap(4.0)
                .center_y()
                .child(
                    rune::icon(&rune_theme_opencode::icons::ARROW_UP, 14.0)
                        .key("download-glyph")
                        .color(ink)
                        .rotate(std::f32::consts::PI),
                )
                .child(text(size).key("label").font(tnum(font)).color(ink))
        })
        .on_press_tx(move |ui: &mut Ui, _| choose(ui, def))
        .build_small_push(p.k)
        .key("choose-model")
        // A ghost's label lines up with the row's other right edges.
        .mr(-9.0)
}
