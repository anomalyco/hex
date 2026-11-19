//! Hex's permission window: consent for audio transcription or microphone
//! recording, scoped to the named recipient (macOS).
//!
//! It is its own floating window (a Rune overlay), not a dialog inside Hex's
//! main window, so it asks whether that window is open, closed, or Hex is
//! only in the menu bar, and it never opens or raises the main window. It
//! shows on the screen under the pointer, takes the keyboard while shown
//! (its panel never activates Hex, so the app in front keeps focus once it
//! hides), and holds no GPU surface while hidden. One ask at a time; it
//! hides at once on an answer, a timeout, or anything else that ends the ask.
//!
//! The dialog reads as the question a person has to answer, in order: what
//! Hex is being asked for, who is asking and how Hex knows, what they get,
//! then the three answers. Allow Once is the one accent; Always Allow sits
//! apart as a Secondary button, so lasting access is never the easiest press,
//! and Deny is the Return default and the first focus (Escape denies too).
//! For [`GUARD`] after it appears, both allows are disabled, so a keystroke
//! or click meant for another app can't grant anything; Deny always works.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rune::{Cx, El, Icon, TextStyle, Tx, col, el, row, spacer, text};
use rune_theme_opencode::icons::{CIRCLE_CHECK, MONITOR, OUTLINE_TEXT, SHIELD};

use super::controls::{ButtonKind, DESCRIPTION, Paint, state_button};
use super::icons::{CLOCK, GLOBE, MIC, WAVE, tile};
use crate::desktop::i18n::{tr, tr_fill};
use crate::local_api::{Answer, Intent, Prompt, Recipient};

/// The dialog's width and padding.
const WIDTH: f32 = 440.0;
const PADDING: f32 = 20.0;
/// The question: one step above the kit's body type, as a dialog's title.
const TITLE: TextStyle = TextStyle::new(17.0, 560.0).lh(22.0).tracking(-0.2);
/// The window: the dialog with room for its shadow. Only the dialog takes
/// the pointer; the clear margin passes clicks through.
pub(crate) const SIZE: (f32, f32) = (520.0, 620.0);
/// How long both allows stay disabled after the window appears, as a
/// browser's permission prompt guards against a keystroke meant elsewhere.
pub(crate) const GUARD: Duration = Duration::from_secs(1);

/// Where an answer goes: the consent gate's `answer(id, answer)`.
pub(crate) type Reply = Arc<dyn Fn(u64, Answer) + Send + Sync>;

/// The permission window's state.
#[derive(Default)]
pub(crate) struct Sheet {
    prompt: Option<Prompt>,
    /// When the current ask appeared.
    shown_at: Option<Instant>,
    /// The guard has passed: the allows work.
    armed: bool,
    reply: Option<Reply>,
    overlay: Option<rune::OverlayHandle<Sheet>>,
}

impl Sheet {
    pub(crate) fn new(reply: Reply) -> Self {
        Self {
            reply: Some(reply),
            ..Self::default()
        }
    }

    /// Shows `prompt`, or hides the window when the ask ended (`None`).
    pub(crate) fn ask(&mut self, prompt: Option<Prompt>, tx: &mut Tx) {
        let Some(prompt) = prompt else {
            self.close();
            return;
        };
        if self.prompt.as_ref().map(|shown| shown.id) == Some(prompt.id) {
            return;
        }
        self.prompt = Some(prompt);
        self.shown_at = Some(Instant::now());
        self.armed = false;
        tx.wake_after(GUARD);
        if let Some(overlay) = &self.overlay {
            #[cfg(target_os = "macos")]
            place(overlay);
            overlay.show();
            overlay.request_keys();
        }
    }

    /// Hides the window: the ask was answered, timed out, or withdrawn.
    fn close(&mut self) {
        self.prompt = None;
        self.shown_at = None;
        self.armed = false;
        if let Some(overlay) = &self.overlay {
            overlay.hide();
        }
    }

    /// Ends the guard once it has run; true when that changed what shows.
    fn arm(&mut self, now: Instant) -> bool {
        if self.armed || !self.shown_at.is_some_and(|shown| now >= shown + GUARD) {
            return false;
        }
        self.armed = true;
        true
    }

    #[cfg(test)]
    fn showing(&self) -> Option<u64> {
        self.prompt.as_ref().map(|prompt| prompt.id)
    }
}

/// Answers the ask shown, and hides the window at once. During the guard an
/// allow does nothing; Deny always answers. One window is one answer.
fn answer(sheet: &mut Sheet, answer: Answer) {
    let Some(prompt) = sheet.prompt.as_ref() else {
        return;
    };
    if answer != Answer::Deny && !sheet.armed {
        return;
    }
    let id = prompt.id;
    sheet.close();
    if let Some(reply) = &sheet.reply {
        reply(id, answer);
    }
}

fn poll(sheet: &mut Sheet, _: f64, _: &mut Tx) -> bool {
    sheet.arm(Instant::now())
}

/// Adds the permission window to the app, hidden, and returns what the
/// consent gate calls as each ask starts (`Some`) and ends (`None`); it may
/// be called from any thread.
#[cfg(target_os = "macos")]
pub(crate) fn install<S: 'static>(
    app: &mut rune::App<S>,
    reply: Reply,
) -> impl Fn(Option<Prompt>) + Send + Sync + 'static {
    use rune::{DisplayTarget, KeyPolicy, OverlayInput, OverlayOptions, OverlaySpec};
    let spec = OverlaySpec::new("Hex permission request", SIZE, view)
        .options(OverlayOptions {
            // Placed for real each time it shows (see [`place`]).
            display: DisplayTarget::UnderPointer,
            input: OverlayInput::Regions,
            keys: KeyPolicy::OnDemand,
            ..Default::default()
        })
        .release_gpu_when_hidden()
        .poll(poll);
    let overlay = app.overlay(spec, Sheet::new(reply));
    let handle = overlay.clone();
    overlay.update(move |sheet, _| sheet.overlay = Some(handle));
    move |prompt| overlay.update(move |sheet, tx| sheet.ask(prompt, tx))
}

/// Centres the window on the visible part of the screen under the pointer,
/// the screen Hex's HUD treats as the person's current one.
#[cfg(target_os = "macos")]
fn place(overlay: &rune::OverlayHandle<Sheet>) {
    let Some((screen, primary_top, _, _)) =
        objc2::MainThreadMarker::new().and_then(crate::rune_hud::pointer_screen)
    else {
        return;
    };
    let (x, y) = centred(&screen.visible, primary_top, SIZE);
    overlay.set_display(rune::DisplayTarget::Primary);
    overlay.set_placement(Some(rune::OverlayPlacement::Rect {
        x,
        y,
        width: SIZE.0,
        height: SIZE.1,
    }));
}

/// The top-left, in Rune's global space (points from the primary screen's
/// top-left, y down), that centres `size` on `visible` (AppKit's space,
/// y up).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn centred(
    visible: &crate::rune_hud::hud_placement::Rect,
    primary_top: f64,
    size: (f32, f32),
) -> (f32, f32) {
    let left = visible.x + (visible.width - f64::from(size.0)) / 2.0;
    let top = visible.y + visible.height - (visible.height - f64::from(size.1)) / 2.0;
    (left as f32, (primary_top - top) as f32)
}

/// What one kind of ask shows: its capability, question, and what the
/// recipient gets.
struct Ask {
    glyph: &'static Icon,
    capability: &'static str,
    title: &'static str,
    lead: &'static str,
    gets: Vec<(&'static Icon, String)>,
}

fn ask_copy(intent: Intent, recipient: &Recipient) -> Ask {
    let page = matches!(recipient, Recipient::Web { .. });
    if intent == Intent::Record {
        Ask {
            glyph: &MIC,
            capability: tr("Microphone"),
            title: tr("Allow microphone recording?"),
            lead: if page {
                tr(
                    "This page wants Hex to record from your microphone. Nothing records until it asks.",
                )
            } else {
                tr(
                    "This app wants Hex to record from your microphone. Nothing records until it asks.",
                )
            },
            gets: vec![
                (
                    &OUTLINE_TEXT,
                    tr("Only the transcript, never the audio").to_owned(),
                ),
                (
                    &CLOCK,
                    if page {
                        tr("Recording stops when the page ends it or stops responding, or anytime in Hex")
                    } else {
                        tr("Recording stops when the app ends it or stops responding, or anytime in Hex")
                    }
                    .to_owned(),
                ),
            ],
        }
    } else {
        Ask {
            glyph: &WAVE,
            capability: tr("Transcription"),
            title: if page {
                tr("Allow this page to transcribe audio?")
            } else {
                tr("Allow this app to transcribe audio?")
            },
            lead: if page {
                tr("This page wants to send Hex audio it already has and get the text back.")
            } else {
                tr("This app wants to send Hex audio it already has and get the text back.")
            },
            gets: vec![
                (
                    &OUTLINE_TEXT,
                    tr("The text of each audio file it sends").to_owned(),
                ),
                (&MIC, tr("Your microphone stays off").to_owned()),
            ],
        }
    }
}

/// Who is asking: the identity Hex checked, foremost, then how it knows,
/// then (for a page) the name the page gave itself, framed as a claim.
fn requester(p: Paint, recipient: &Recipient) -> El {
    let c = &p.k.c;
    let (glyph, kind, proof, proven): (&'static Icon, _, _, _) = match recipient {
        Recipient::Native { team, .. } => (
            &MONITOR,
            tr("App"),
            match team {
                Some(team) => tr_fill("Code signature verified · team {}", team),
                None => tr("Code signature verified · no team").to_owned(),
            },
            true,
        ),
        Recipient::Web { .. } => (
            &GLOBE,
            tr("Website"),
            tr("On your list in Labs › Browser access").to_owned(),
            false,
        ),
    };
    let identity = recipient.label();
    let mut said = format!("{kind}: {identity}. {proof}.");
    let mut copy = col()
        .key("requester-copy")
        .grow(1.0)
        .shrink()
        .gap(2.0)
        .child(
            text(kind)
                .key("requester-kind")
                .font(p.k.ty.caption())
                .color(p.muted())
                .a11y_hidden(),
        )
        .child(
            text(identity)
                .key("recipient-value")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .wrap()
                .a11y_hidden(),
        )
        .child(
            row()
                .key("proof")
                .gap(5.0)
                .pt(2.0)
                .center_y()
                .child(
                    rune::icon(if proven { &CIRCLE_CHECK } else { &SHIELD }, 14.0)
                        .key("proof-icon")
                        .color(if proven { c.success.icon } else { c.icon_muted })
                        .a11y_hidden(),
                )
                .child(
                    text(proof)
                        .key("signing-note")
                        .font(DESCRIPTION)
                        .color(p.muted())
                        .wrap()
                        .shrink()
                        .a11y_hidden(),
                ),
        );
    if let Some(name) = recipient.claimed_name() {
        let claim = tr_fill("Calls itself “{}” · not verified", name);
        said.push(' ');
        said.push_str(&claim);
        said.push('.');
        copy = copy.child(
            text(claim)
                .key("client-name")
                .font(DESCRIPTION)
                .color(p.faint())
                .wrap()
                .a11y_hidden(),
        );
    }
    row()
        .key("recipient")
        .role("group")
        .label(tr("Requested by"))
        .description(said)
        .items_start()
        .gap(12.0)
        .px(12.0)
        .py(12.0)
        .radius(p.k.radius.lg)
        .fill(c.layer_01)
        .border(p.hair, c.border_weak)
        .child(tile(p, glyph, 32.0))
        .child(copy)
}

/// What the recipient gets if allowed, one line each.
fn gets(p: Paint, lines: Vec<(&'static Icon, String)>) -> El {
    let label = tr("If you allow");
    let mut list = col().key("gets").role("list").label(label).gap(8.0).child(
        text(label)
            .key("gets-label")
            .font(p.k.ty.small_medium)
            .color(p.muted())
            .a11y_hidden(),
    );
    for (index, (glyph, line)) in lines.into_iter().enumerate() {
        list = list.child(
            row()
                .key(format!("get{index}"))
                .role("listitem")
                .label(line.clone())
                .gap(10.0)
                .center_y()
                .child(
                    rune::icon(glyph, 16.0)
                        .key("icon")
                        .color(p.k.c.icon_muted)
                        .a11y_hidden(),
                )
                .child(
                    text(line)
                        .key("copy")
                        .font(p.k.ty.ui)
                        .color(p.text())
                        .wrap()
                        .shrink()
                        .a11y_hidden(),
                ),
        );
    }
    list
}

/// The ask's capability, as a mark and a name, under Hex's own label so
/// the dialog never passes for one of the system's.
fn capability(p: Paint, ask: &Ask) -> El {
    let c = &p.k.c;
    row()
        .key("capability")
        .gap(10.0)
        .center_y()
        .child(
            el().key("capability-mark")
                .size(36.0, 36.0)
                .center()
                .radius(p.k.radius.lg)
                .fill(c.bg_contrast)
                .a11y_hidden()
                .child(
                    rune::icon(ask.glyph, 18.0)
                        .key("glyph")
                        .color(c.icon_contrast),
                ),
        )
        .child(
            col()
                .key("capability-copy")
                .child(
                    text(tr("Hex permission request"))
                        .key("eyebrow")
                        .font(p.k.ty.caption())
                        .color(p.muted()),
                )
                .child(
                    text(ask.capability)
                        .key("capability-name")
                        .font(p.k.ty.ui_medium)
                        .color(p.text()),
                ),
        )
}

/// The three answers. Allow Once is the accent and Deny the Return default
/// and first focus; Always Allow stands apart on the leading edge, with what
/// it means under it. Both allows are disabled until `armed`.
fn footer(p: Paint, armed: bool) -> El {
    col()
        .key("footer-area")
        .px(PADDING)
        .pt(14.0)
        .pb(14.0)
        .gap(10.0)
        .fill(p.k.c.layer_01)
        .child(
            row()
                .key("footer")
                .gap(8.0)
                .center_y()
                .child(state_button(
                    p,
                    "browser-connect-always",
                    tr("Always Allow"),
                    ButtonKind::Secondary,
                    !armed,
                    |sheet: &mut Sheet, _| answer(sheet, Answer::Always),
                ))
                .child(spacer())
                .child(
                    state_button(
                        p,
                        "browser-connect-deny",
                        tr("Deny"),
                        ButtonKind::Secondary,
                        false,
                        |sheet: &mut Sheet, _| answer(sheet, Answer::Deny),
                    )
                    .default_button()
                    .autofocus(),
                )
                .child(state_button(
                    p,
                    "browser-connect-allow",
                    tr("Allow Once"),
                    ButtonKind::Primary,
                    !armed,
                    |sheet: &mut Sheet, _| answer(sheet, Answer::Once),
                )),
        )
        .child(
            text(tr(
                "Always Allow lasts until you revoke it in Labs › Browser access.",
            ))
            .key("limits")
            .font(p.k.ty.caption())
            .color(p.faint())
            .wrap(),
        )
}

/// The permission window: the dialog centred in a clear window, while an
/// ask waits; nothing while none does.
pub(crate) fn view(sheet: &Sheet, cx: &mut Cx<Sheet>) -> El {
    let p = Paint::new(cx.system_dark(), cx.env().scale);
    let root = el().key("consent-window").role("none").w_full().h_full();
    let Some(prompt) = sheet.prompt.as_ref() else {
        return root;
    };
    root.center().child(dialog(p, prompt, sheet.armed))
}

fn dialog(p: Paint, prompt: &Prompt, armed: bool) -> El {
    let c = &p.k.c;
    let ask = ask_copy(prompt.intent, &prompt.recipient);
    let question = col()
        .key("question")
        .gap(4.0)
        .child(
            text(ask.title)
                .key("title")
                .role("heading")
                .level(1)
                .font(TITLE)
                .color(p.text())
                .wrap(),
        )
        .child(
            text(ask.lead)
                .key("body")
                .font(p.k.ty.ui)
                .color(p.muted())
                .wrap(),
        );
    let content = col()
        .key("content")
        .pt(PADDING)
        .px(PADDING)
        .pb(18.0)
        .gap(16.0)
        .child(capability(p, &ask))
        .child(question)
        .child(requester(p, &prompt.recipient))
        .child(gets(p, ask.gets));
    let mut panel = col()
        .key("browser-connect-dialog")
        .role("dialog")
        .label(ask.title)
        .description(ask.lead)
        .w(WIDTH)
        .radius(p.k.radius.xl)
        .clip_rounded(p.k.radius.xl)
        .fill(c.bg_base)
        .border(p.hair, c.border_weak)
        // Only the dialog takes the pointer; presses inside it stay inside.
        .hit()
        .on_press(|_: &mut Sheet| {})
        .child(content)
        .child(el().key("rule").h(p.hair).fill(c.border_weak).a11y_hidden())
        .child(footer(p, armed));
    for shadow in &p.k.elevation.overlay {
        panel = panel.box_shadow(*shadow);
    }
    // Keys stay inside and Escape denies; there is nothing outside to click.
    panel.modal(false, |sheet: &mut Sheet| answer(sheet, Answer::Deny))
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::local_api::{Intent, Recipient};

    fn ask() -> Prompt {
        Prompt {
            id: 7,
            intent: Intent::Transcribe,
            recipient: Recipient::Web {
                origin: "https://notes.example".into(),
                client_name: Some("Field Notes".into()),
            },
        }
    }

    fn record_ask() -> Prompt {
        Prompt {
            id: 9,
            intent: Intent::Record,
            recipient: Recipient::Native {
                identifier: "com.example.recorder".into(),
                cdhash: "fixture-digest".into(),
                team: Some("ABCDE12345".into()),
            },
        }
    }

    type Replies = Arc<Mutex<Vec<(u64, Answer)>>>;

    /// The permission window showing `prompt`, rendered, with its guard
    /// passed when `armed`, and the answers it sends.
    fn shown(prompt: Prompt, armed: bool, dark: bool) -> (rune::Runtime<Sheet>, Replies) {
        let replies: Replies = Arc::default();
        let sent = replies.clone();
        let sheet = Sheet::new(Arc::new(move |id, answer| {
            sent.lock().unwrap().push((id, answer));
        }));
        let mut rt = crate::rune_ui::offscreen::runtime(
            sheet,
            view,
            SIZE,
            rune::Env {
                system_dark: dark,
                scale: 2.0,
            },
            rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
        );
        rt.update(move |sheet, tx| sheet.ask(Some(prompt), tx));
        if armed {
            rt.update(|sheet, _| {
                sheet.shown_at = Some(Instant::now() - GUARD);
                assert!(sheet.arm(Instant::now()));
            });
        }
        rt.settle_all();
        rt.frame(0.0);
        (rt, replies)
    }

    fn copy(rt: &rune::Runtime<Sheet>) -> Vec<String> {
        rt.elements().into_iter().filter_map(|e| e.text).collect()
    }

    fn press(rt: &mut rune::Runtime<Sheet>, key: &str) {
        let (x, y) = rt.find(key).unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
    }

    fn key(rt: &mut rune::Runtime<Sheet>, key: rune::Key) {
        rt.key(rune::KeyEvent {
            key,
            mods: rune::Mods::default(),
        });
        rt.frame(0.0);
    }

    #[test]
    fn each_answer_is_sent_once_and_hides_the_window() {
        for prompt in [ask(), record_ask()] {
            for (button, label, expected) in [
                ("browser-connect-deny", "Deny", Answer::Deny),
                ("browser-connect-allow", "Allow Once", Answer::Once),
                ("browser-connect-always", "Always Allow", Answer::Always),
            ] {
                let (mut rt, replies) = shown(prompt.clone(), true, true);
                let copy = copy(&rt);
                assert!(copy.iter().any(|s| s == label));
                assert!(copy.iter().any(|s| s == "Hex permission request"));
                assert!(!copy.iter().any(|s| s.to_lowercase().contains("upload")));
                press(&mut rt, button);
                assert_eq!(*replies.lock().unwrap(), [(prompt.id, expected)]);
                assert_eq!(rt.state.showing(), None, "{label}");
                assert!(rt.find("browser-connect-dialog").is_none());
                // A late second press answers nothing.
                answer(&mut rt.state, Answer::Deny);
                assert_eq!(replies.lock().unwrap().len(), 1);
            }
        }
    }

    /// Right after the window appears, both allows are disabled (no Tab
    /// stop, no press) while Deny works; once the guard passes they work.
    #[test]
    fn the_allows_wait_out_the_guard_and_deny_never_does() {
        let (mut rt, replies) = shown(record_ask(), false, false);
        let focusable = |rt: &rune::Runtime<Sheet>, key: &str| {
            rt.elements()
                .into_iter()
                .find(|e| e.key.as_deref() == Some(key))
                .is_some_and(|e| e.focusable)
        };
        assert!(!focusable(&rt, "browser-connect-allow"));
        assert!(!focusable(&rt, "browser-connect-always"));
        assert!(focusable(&rt, "browser-connect-deny"));
        press(&mut rt, "browser-connect-allow");
        press(&mut rt, "browser-connect-always");
        answer(&mut rt.state, Answer::Once);
        assert!(replies.lock().unwrap().is_empty());
        assert_eq!(rt.state.showing(), Some(9));

        // Not yet: the guard runs its full length.
        assert!(!rt.state.arm(Instant::now()));
        let later = rt.state.shown_at.unwrap() + GUARD;
        assert!(rt.state.arm(later));
        rt.settle_all();
        rt.frame(0.0);
        assert!(focusable(&rt, "browser-connect-allow"));
        press(&mut rt, "browser-connect-allow");
        assert_eq!(*replies.lock().unwrap(), [(9, Answer::Once)]);

        let (mut rt, replies) = shown(record_ask(), false, false);
        press(&mut rt, "browser-connect-deny");
        assert_eq!(*replies.lock().unwrap(), [(9, Answer::Deny)]);
    }

    /// Return presses the default, Deny, and Escape denies too, during the
    /// guard and after: neither key can grant anything.
    #[test]
    fn return_and_escape_deny() {
        for armed in [false, true] {
            for pressed in [rune::Key::Enter, rune::Key::Escape] {
                let (mut rt, replies) = shown(record_ask(), armed, true);
                key(&mut rt, pressed);
                assert_eq!(*replies.lock().unwrap(), [(9, Answer::Deny)]);
                assert_eq!(rt.state.showing(), None);
            }
        }
    }

    /// The ask ending without a button (a timeout, or the gate withdrawing
    /// it) hides the window; the next ask shows with a fresh guard.
    #[test]
    fn an_ended_ask_hides_and_the_next_starts_its_own_guard() {
        let (mut rt, replies) = shown(ask(), true, true);
        rt.update(|sheet, tx| sheet.ask(None, tx));
        rt.frame(0.0);
        assert_eq!(rt.state.showing(), None);
        assert!(rt.find("browser-connect-dialog").is_none());
        rt.update(|sheet, tx| sheet.ask(Some(record_ask()), tx));
        rt.settle_all();
        rt.frame(0.0);
        assert_eq!(rt.state.showing(), Some(9));
        answer(&mut rt.state, Answer::Always);
        assert!(replies.lock().unwrap().is_empty(), "guarded again");
        // The same ask published twice keeps its guard running.
        let first = rt.state.shown_at;
        rt.update(|sheet, tx| sheet.ask(Some(record_ask()), tx));
        assert_eq!(rt.state.shown_at, first);
    }

    #[test]
    fn lasting_access_is_not_the_promoted_answer() {
        let (rt, _) = shown(ask(), true, false);
        let (always, _) = rt.find("browser-connect-always").unwrap();
        let (deny, _) = rt.find("browser-connect-deny").unwrap();
        let (once, _) = rt.find("browser-connect-allow").unwrap();
        assert!(always < deny && deny < once);
        let focused = rt.elements().into_iter().find(|e| e.focused);
        assert_eq!(
            focused.and_then(|e| e.key).as_deref(),
            Some("browser-connect-deny")
        );
    }

    #[test]
    fn a_page_is_named_by_its_origin_and_its_own_name_is_only_a_claim() {
        let (rt, _) = shown(ask(), true, true);
        let copy = copy(&rt);
        assert!(copy.iter().any(|s| s == "https://notes.example"));
        assert!(
            copy.iter()
                .any(|s| s == "Calls itself “Field Notes” · not verified")
        );
        assert!(!copy.iter().any(|s| s.contains("Code signature")));
    }

    #[test]
    fn a_recording_ask_names_the_verified_native_identity() {
        let (rt, _) = shown(record_ask(), true, true);
        let copy = copy(&rt);
        assert!(copy.iter().any(|s| s == "com.example.recorder"));
        assert!(
            copy.iter()
                .any(|s| s == "Code signature verified · team ABCDE12345")
        );
        assert!(copy.iter().any(|s| s
            == "This app wants Hex to record from your microphone. Nothing records until it asks."));
    }

    /// The window centres on the visible part of the pointer's screen.
    #[test]
    fn the_window_centres_on_the_visible_screen() {
        use crate::rune_hud::hud_placement::Rect;
        // A 1512×982 primary screen with a 37-point menu bar and no Dock.
        let visible = Rect {
            x: 0.0,
            y: 0.0,
            width: 1512.0,
            height: 945.0,
        };
        let (x, y) = centred(&visible, 982.0, SIZE);
        assert_eq!(x, (1512.0 - SIZE.0) / 2.0);
        assert_eq!(y, 37.0 + (945.0 - SIZE.1) / 2.0);
    }

    /// Renders the permission window to `HEX_CONSENT_SNAPS/standalone-*.png`
    /// over a plain desktop grey, to look at.
    #[test]
    #[ignore = "set HEX_CONSENT_SNAPS to a folder; needs a GPU"]
    fn snapshots() {
        let folder = std::path::PathBuf::from(std::env::var("HEX_CONSENT_SNAPS").unwrap());
        let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).unwrap();
        for (name, prompt, armed, dark) in [
            ("record-light", record_ask(), true, false),
            ("record-dark", record_ask(), true, true),
            ("transcribe-light", ask(), true, false),
            ("transcribe-dark", ask(), true, true),
            ("guard-light", record_ask(), false, false),
        ] {
            let (mut rt, _) = shown(prompt, armed, dark);
            rt.frame(0.0);
            let (w, h) = ((SIZE.0 * 2.0) as u32, (SIZE.1 * 2.0) as u32);
            let mut renderer = crate::rune_ui::offscreen::renderer(&gpu.device);
            let target = crate::rune_ui::offscreen::target(&gpu.device, w, h);
            renderer.upload_images(&gpu.device, &gpu.queue, &mut rt.images);
            renderer.prepare(
                &gpu.device,
                &gpu.queue,
                &mut rt.text.atlas,
                &rt.instances,
                (w, h),
            );
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let backdrop = if dark { 0.08 } else { 0.62 };
            renderer.draw(
                &mut encoder,
                &target.view,
                [backdrop, backdrop, backdrop, 1.0],
            );
            target.copy(&mut encoder);
            gpu.queue.submit([encoder.finish()]);
            let path = folder.join(format!("standalone-{name}.png"));
            rune::render::write_png(&path, w, h, &target.read(&gpu.device)).unwrap();
        }
    }
}
