//! The Output pane (the `modes` experiment): what Hex does to every
//! dictation before it pastes it, and what some apps use instead, read from
//! `dictation_processing`. The pane only shows the steps; the person's agent
//! changes them through `hex settings`, so the pane leads with Edit with
//! agent, which copies a prompt saying where the steps live and what each
//! can be.

use rune::{El, LoopSpec, Shadow, TextEdit, Tx, col, el, row, text};
use rune_kit::{Button, ButtonVariant, TextInput};

use super::Ui;
use super::controls::{
    ButtonKind, CardRow, DESCRIPTION, METRICS, Paint, PushSize, button, card, folding, note, page,
};
use super::lists::{Tone, tone_tag};
use super::modes::{application_icon, monogram};
use crate::app_settings::DictationMode;
use crate::desktop_i18n::{tr, tr_fill};
use crate::text_replacements::ReplacementSet;

/// The agent card's halo breathes in and out this slowly, from nothing to
/// most of its faint strength; Reduce Motion holds it off.
/// The agent mark's halo breathes, and its diamond turns once every
/// `SPIN` seconds; Reduce Motion stills both.
const GLOW: LoopSpec = LoopSpec::opacity(4.0, 0.0, 0.25, 1.0);
const SPIN: LoopSpec = LoopSpec::rotation(16.0);
/// Each app rule's folding steps, by its place in the list.
const RULE_STEPS: [&str; 8] = [
    "rule0-steps",
    "rule1-steps",
    "rule2-steps",
    "rule3-steps",
    "rule4-steps",
    "rule5-steps",
    "rule6-steps",
    "rule7-steps",
];
/// A step's number sits in a disc this wide, on the row text's edge.
const STEP_BADGE: f32 = 20.0;

#[derive(Default)]
pub(super) struct OutputPane {
    /// The Try field's sentence.
    pub(super) trial: TextEdit,
    /// Edit with agent copied its prompt, until the pane is left.
    pub(super) prompt_copied: bool,
    /// The app rule opened to show its steps.
    pub(super) expanded: Option<usize>,
}

/// What runs a step: Hex itself, a script from the person's config, or
/// OpenCode rewriting.
#[derive(Clone, Copy, PartialEq)]
enum Runner {
    BuiltIn,
    Script,
    Rewrite,
}

struct Step {
    title: String,
    detail: Option<String>,
    runner: Runner,
}

/// A mode's steps in the order they run: its corrections, its
/// transformations, then OpenCode rewriting when it is on.
fn steps(ui: &Ui, mode: &DictationMode) -> Vec<Step> {
    let mut steps = Vec::new();
    if !mode.replacements.is_empty() {
        let pairs = mode
            .replacements
            .iter()
            .filter(|pair| !pair.matched_phrase.trim().is_empty())
            .map(|pair| {
                let output = if pair.output.is_empty() {
                    tr("(nothing)")
                } else {
                    pair.output.as_str()
                };
                format!("{} → {output}", pair.matched_phrase)
            })
            .collect::<Vec<_>>()
            .join(" · ");
        steps.push(Step {
            title: tr("Replace words").into(),
            detail: (!pairs.is_empty()).then_some(pairs),
            runner: Runner::BuiltIn,
        });
    }
    for id in &mode.transformations {
        steps.push(match builtin(id) {
            Some((title, detail)) => Step {
                title: title.into(),
                detail: Some(detail.into()),
                runner: Runner::BuiltIn,
            },
            None => {
                let script = ui
                    .modes
                    .status
                    .transformations
                    .iter()
                    .find(|transformation| transformation.id == *id);
                Step {
                    title: script.map_or_else(|| id.clone(), |script| script.name.clone()),
                    detail: script.and_then(|script| script.description.clone()),
                    runner: Runner::Script,
                }
            }
        });
    }
    if mode.post_processing.enabled {
        steps.push(Step {
            title: tr("Rewrite with OpenCode").into(),
            detail: mode.post_processing.model.clone(),
            runner: Runner::Rewrite,
        });
    }
    steps
}

/// A built-in transformation's name and what it does.
fn builtin(id: &str) -> Option<(&'static str, &'static str)> {
    match id {
        "lowercase" => Some((tr("Lowercase"), tr("Lowercases everything"))),
        "no-trailing-punctuation" => Some((tr("No final period"), tr("Drops the last . ! or ?"))),
        "spongebob-case" => Some((tr("SpongeBob case"), tr("aLtErNaTeS every letter"))),
        _ => None,
    }
}

/// The Try field's sentence after every dictation's corrections and
/// built-in steps. Scripts and rewriting run only when dictating.
fn preview(mode: &DictationMode, input: &str) -> String {
    let mut output = ReplacementSet::new(&mode.replacements).replace(input);
    for id in &mode.transformations {
        if let Some(next) = crate::personal_commands::apply_builtin_transformation(id, &output) {
            output = next;
        }
    }
    output
}

// ---------------------------------------------------------------------------
// View

pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let processing = &ui.settings.dictation_processing;
    let mut content = page("output-content")
        .child(agent_card(ui, p))
        .child(every_dictation(ui, p, &processing.default_mode))
        .child(try_card(ui, p, &processing.default_mode));
    if !processing.modes.is_empty() {
        content = content.child(some_apps(ui, p, &processing.modes));
    }
    super::scroll_body("output-scroll", content)
}

/// The pane's one action: the agent writes the steps, so this card is how
/// to change them.
fn agent_card(ui: &Ui, p: Paint) -> El {
    let c = &p.k.c;
    let radius = p.k.radius.lg;
    let glow = el()
        .key("glow")
        .pin(Some(0.0), Some(0.0), Some(0.0), Some(0.0))
        .radius(p.k.radius.md)
        .box_shadow(Shadow {
            blur: 10.0,
            spread: 0.0,
            color: if p.k.dark {
                c.info.border.alpha(0.7)
            } else {
                c.info.icon.alpha(0.35)
            },
            ..Shadow::default()
        })
        .looping(GLOW);
    let diamond = rune::icon(&rune_kit::glyphs::SPARKLE, 16.0)
        .key("diamond")
        .color(if p.k.dark { p.text() } else { c.info.fg })
        .looping(SPIN);
    let mark = row().key("mark").size(32.0, 32.0).child(glow).child(
        row()
            .key("tile")
            .size(32.0, 32.0)
            .center()
            .radius(p.k.radius.md)
            .fill(c.info.border)
            .child(diamond),
    );
    let detail = if ui.output.prompt_copied {
        tr("Copied. Paste it into your agent and say what you want")
    } else {
        tr("Tell it what you want, like “no final period in Slack”")
    };
    let copy = col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .gap(2.0)
        .child(
            text(tr("Your agent writes these steps"))
                .key("title")
                .font(p.k.ty.ui_medium)
                .color(p.text()),
        )
        .child(
            text(detail)
                .key("detail")
                .font(DESCRIPTION)
                .color(c.info.fg)
                .wrap(),
        );
    let open = Button::new(tr("Open file"))
        .variant(ButtonVariant::Ghost)
        .on_press(open_settings_file)
        .build_push(p.k)
        .key("open-settings-file");
    let edit = button(
        p,
        "edit-with-agent",
        if ui.output.prompt_copied {
            tr("Copied")
        } else {
            tr("Edit with agent")
        },
        ButtonKind::Primary,
        |ui: &mut Ui, tx: &mut Tx| {
            tx.copy(agent_prompt());
            ui.output.prompt_copied = true;
        },
    );
    row()
        .key("agent-card")
        .px(METRICS.row_px)
        .py(14.0)
        .gaps(8.0, 14.0)
        .center_y()
        .flex_wrap()
        .radius(radius)
        .fill(c.info.bg.alpha(if p.k.dark { 0.45 } else { 1.0 }))
        .border(
            p.hair.max(0.5),
            c.info.border.alpha(if p.k.dark { 0.7 } else { 1.0 }),
        )
        .child(
            row()
                .key("lead")
                .w(200.0)
                .grow(1.0)
                .gap(12.0)
                .center_y()
                .child(mark)
                .child(copy),
        )
        .child(
            row()
                .key("actions")
                .gap(4.0)
                .center_y()
                .child(open)
                .child(edit),
        )
}

/// The steps every dictation goes through, numbered in order.
fn every_dictation(ui: &Ui, p: Paint, mode: &DictationMode) -> El {
    let steps = steps(ui, mode);
    let rows: Vec<CardRow> = if steps.is_empty() {
        vec![
            note(
                "no-steps",
                tr("No steps yet. Hex pastes your words as it heard them"),
                p.muted(),
            )
            .into(),
        ]
    } else {
        steps
            .iter()
            .enumerate()
            .map(|(index, step)| step_row(p, &format!("step{index}"), index, step).into())
            .collect()
    };
    labeled(
        p,
        "every-dictation",
        tr("Every dictation"),
        tr("Runs top to bottom"),
        rows,
    )
}

/// A section label with a faint hint at its end, over a card.
fn labeled(p: Paint, key: &str, title: &str, hint: &str, rows: Vec<CardRow>) -> El {
    col()
        .key(key)
        .gap(METRICS.label_gap)
        .child(
            row()
                .key("header-row")
                .mx(METRICS.row_px)
                .gap(8.0)
                .center_y()
                .child(p.section_label(title).grow(1.0))
                .child(text(hint).key("hint").font(p.k.ty.small).color(p.faint())),
        )
        .child(card(p, "card", rows))
}

fn step_row(p: Paint, key: &str, index: usize, step: &Step) -> El {
    let c = &p.k.c;
    let badge = row()
        .key("number")
        .size(STEP_BADGE, STEP_BADGE)
        .center()
        .radius(STEP_BADGE / 2.0)
        .fill(c.layer_02)
        .child(
            text((index + 1).to_string())
                .key("digit")
                .font(p.k.ty.small_medium)
                .color(p.muted()),
        );
    let tag = match step.runner {
        Runner::BuiltIn => tone_tag(p, "runner", tr("Built in"), Tone::Neutral),
        Runner::Script => tone_tag(p, "runner", "JS", Tone::Accent),
        Runner::Rewrite => tone_tag(p, "runner", "AI", Tone::Accent),
    };
    row()
        .key(key)
        .min_h(METRICS.row_h_described)
        .px(METRICS.row_px)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .label(step.title.clone())
        .child(badge)
        .child(
            col()
                .key("copy")
                .grow(1.0)
                .shrink()
                .gap(2.0)
                .child(
                    text(step.title.clone())
                        .key("title")
                        .font(p.k.ty.ui)
                        .color(p.text()),
                )
                .children(step.detail.as_ref().map(|detail| {
                    text(detail.clone())
                        .key("detail")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap()
                })),
        )
        .child(tag)
}

/// A sentence in, and what Hex would paste for it out.
fn try_card(ui: &Ui, p: Paint, mode: &DictationMode) -> El {
    let c = &p.k.c;
    let input = TextInput::new(&ui.output.trial)
        .placeholder(tr("Type what you might say"))
        .width(None)
        .on_edit(|ui: &mut Ui| &mut ui.output.trial)
        .build(p.k)
        .key("try-input");
    let trial = ui.output.trial.text.trim();
    let (result, ink) = if trial.is_empty() {
        (tr("What Hex pastes").to_string(), p.faint())
    } else {
        (preview(mode, trial), p.text())
    };
    let result = row()
        .key("try-result")
        .min_h(28.0)
        .px(8.0)
        .py(4.0)
        .center_y()
        .radius(p.k.radius.md)
        .fill(c.layer_01)
        .border(p.hair, c.border_muted)
        .child(
            text(result)
                .key("text")
                .font(p.k.ty.ui_compact)
                .color(ink)
                .wrap()
                .grow(1.0),
        );
    card(
        p,
        "try-card",
        vec![
            row()
                .key("try")
                .px(METRICS.row_px)
                .py(12.0)
                .gap(10.0)
                .center_y()
                .child(
                    text(tr("Try"))
                        .key("label")
                        .font(p.k.ty.ui)
                        .color(p.muted()),
                )
                .child(col().key("in").w(0.0).grow(1.0).child(input))
                .child(
                    rune::icon(&rune_kit::glyphs::ARROW_RIGHT, 16.0)
                        .key("arrow")
                        .color(c.icon_faint),
                )
                .child(col().key("out").w(0.0).grow(1.0).child(result)),
        ],
    )
}

/// Rules whose steps run instead of Every dictation's in the apps and
/// websites they name, each opening to show its steps.
fn some_apps(ui: &Ui, p: Paint, modes: &[DictationMode]) -> El {
    let mut rows: Vec<CardRow> = Vec::new();
    for (index, mode) in modes.iter().enumerate() {
        let steps = steps(ui, mode);
        rows.push(app_row(ui, p, index, mode, &steps).into());
        let open = ui.output.expanded == Some(index);
        rows.push(folding(
            RULE_STEPS.get(index).copied().unwrap_or("rule-steps"),
            steps
                .iter()
                .filter(|_| open)
                .enumerate()
                .map(|(step, item)| {
                    step_row(p, &format!("rule{index}-step{step}"), step, item)
                        .pl(28.0 + METRICS.row_px)
                }),
        ));
    }
    labeled(
        p,
        "some-apps",
        tr("In some apps"),
        tr("Used there instead"),
        rows,
    )
}

fn app_row(ui: &Ui, p: Paint, index: usize, mode: &DictationMode, steps: &[Step]) -> El {
    let c = &p.k.c;
    let places: Vec<&str> = mode
        .applications
        .iter()
        .chain(&mode.browser_hosts)
        .map(String::as_str)
        .collect();
    let title = if places.is_empty() {
        mode.name.clone()
    } else {
        places.join(", ")
    };
    let summary = if steps.is_empty() {
        tr("Pastes your words as Hex heard them").to_string()
    } else {
        tr_fill(
            "Only: {}",
            &steps
                .iter()
                .map(|step| step.title.as_str())
                .collect::<Vec<_>>()
                .join(" · "),
        )
    };
    let icon = mode
        .applications
        .first()
        .and_then(|name| ui.modes.named_icon(name))
        .map_or_else(
            || monogram(p, places.first().copied().unwrap_or(&mode.name)),
            |icon| application_icon("icon", &icon, 24.0),
        );
    let open = ui.output.expanded == Some(index);
    let chevron = if open {
        p.k.glyph.chevron_down
    } else {
        &rune_theme_opencode::icons::CHEVRON_RIGHT
    };
    row()
        .key(format!("rule{index}"))
        .role("button")
        .label(title.clone())
        .focusable(true)
        .focus_ring(2.0, -2.0, c.border_focus)
        .min_h(METRICS.row_h_described)
        .px(METRICS.row_px)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .radius(p.k.radius.lg)
        .hover(c.hover)
        .press(c.pressed)
        .on_press(move |ui: &mut Ui| {
            ui.output.expanded = if ui.output.expanded == Some(index) {
                None
            } else {
                Some(index)
            };
        })
        .child(icon)
        .child(
            col()
                .key("copy")
                .grow(1.0)
                .shrink()
                .gap(2.0)
                .child(
                    text(title)
                        .key("title")
                        .font(p.k.ty.ui)
                        .color(p.text())
                        .truncate(),
                )
                .child(
                    text(summary)
                        .key("summary")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .truncate(),
                ),
        )
        .child(rune::icon(chevron, 16.0).key("chevron").color(c.icon_muted))
}

// ---------------------------------------------------------------------------
// Actions

fn open_settings_file(ui: &mut Ui) {
    if !ui.live() {
        return;
    }
    let opened = crate::app_settings::path()
        .and_then(|path| crate::opencode_catalog::open_url(&path.display().to_string()));
    if let Err(error) = opened {
        ui.error = Some(format!("{error:#}"));
    }
}

/// What Edit with agent copies: where the steps live, their shape, the
/// built-in ones, and how to change them while Hex runs, so any coding
/// agent can do it.
fn agent_prompt() -> String {
    let path = crate::app_settings::path().map_or_else(
        |_| "Hex's settings.json".into(),
        |path| path.display().to_string(),
    );
    format!(
        "I use Hex for dictation. Change what it does to my words before it pastes them.\n\
         \n\
         The steps live in {path} under `dictation_processing`:\n\
         - `default_mode` runs on every dictation: `replacements` (a list of \
         {{\"matched_phrase\", \"output\"}}), then `transformations` (ids, in order).\n\
         - `modes` are rules for some apps: each names `applications` (app names) or \
         `browser_hosts` (sites), and its own `replacements` and `transformations` run \
         there instead.\n\
         \n\
         Built-in transformations: \"lowercase\", \"no-trailing-punctuation\", \
         \"spongebob-case\". Others are functions in ~/.config/hex/hex.config.ts \
         (run `hex commands init` for its typings). Leave `post_processing.enabled` false.\n\
         \n\
         Read the current value with `hex settings get dictation_processing`, then \
         change it with `hex settings set dictation_processing.default_mode '<json>'` \
         (or `dictation_processing.modes`), which Hex applies at once. Don't edit the \
         file while Hex runs.\n\
         \n\
         What I want: "
    )
}

// ---------------------------------------------------------------------------
// Fixture

/// Every dictation corrects words, lowercases, and drops the final period,
/// then a script; chat apps add one more step.
pub(super) fn fixture(ui: &mut Ui) {
    use crate::personal_commands::StatusTransformation;
    use crate::text_replacements::TextReplacement;
    super::modes::fixture(ui);
    let pair = |phrase: &str, output: &str| TextReplacement {
        matched_phrase: phrase.into(),
        output: output.into(),
    };
    let global = &mut ui.settings.dictation_processing.default_mode;
    global.replacements = vec![
        pair("open code", "OpenCode"),
        pair("hex app", "Hex"),
        pair("um", ""),
    ];
    global.transformations = vec![
        "lowercase".into(),
        "no-trailing-punctuation".into(),
        "smart-quotes".into(),
    ];
    ui.modes.status.transformations.push(StatusTransformation {
        id: "smart-quotes".into(),
        name: "Smart quotes".into(),
        description: Some("Curly quotes around quoted words".into()),
    });
    let modes = &mut ui.settings.dictation_processing.modes;
    modes[0].post_processing.enabled = false;
    modes[0].transformations = vec!["smart-punctuation".into()];
    modes[1].applications = vec!["WezTerm".into(), "Zed".into()];
    modes[1].replacements = vec![pair("dash dash", "--")];
    ui.output.trial = TextEdit::new("Um, open code shipped the hex app today.".to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_runs_corrections_then_built_in_steps_in_order() {
        let mode = DictationMode {
            replacements: vec![crate::text_replacements::TextReplacement {
                matched_phrase: "open code".into(),
                output: "OpenCode".into(),
            }],
            transformations: vec![
                "lowercase".into(),
                "a-script".into(),
                "no-trailing-punctuation".into(),
            ],
            ..DictationMode::default()
        };
        assert_eq!(preview(&mode, "Open code shipped."), "opencode shipped");
    }
}
