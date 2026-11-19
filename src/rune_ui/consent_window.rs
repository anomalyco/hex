//! Hex's permission window: consent for audio transcription or microphone
//! recording, scoped to the named recipient, on every OS.
//!
//! It is its own floating window (a Rune overlay), not a dialog inside Hex's
//! main window, so it asks whether that window is open, closed, or Hex is
//! only in the menu bar or tray, and it never opens or raises the main
//! window. It shows on the screen under the pointer, asks for the keyboard
//! while shown (on macOS its panel never activates Hex, so the app in front
//! keeps focus once it hides), and holds no GPU surface while hidden. One ask
//! at a time; it hides at once on an answer, a timeout, or anything else
//! that ends the ask. Where the overlay can't open (a Wayland compositor
//! without layer-shell has none that takes keys), the ask answers
//! unavailable rather than waiting unseen.
//!
//! The dialog reads as the question a person has to answer, in order: what
//! Hex is being asked for, who is asking and how Hex knows, what they get,
//! then the three answers. Allow Once is the one accent; Always Allow sits
//! apart as a Secondary button, so lasting access is never the easiest press,
//! and Deny is the Return default and the first focus (Escape denies too).
//! For [`GUARD`] after it first draws on screen, and again after every key
//! pressed while it shows, both allows are disabled, so a keystroke or click
//! meant for another app can't grant anything (typing that lands here keeps
//! them disabled); Deny always works, and after a pause the keyboard can
//! still reach and press an allow.

use std::sync::Arc;
use std::time::{Duration, Instant};

use rune::{Cx, El, Icon, TextStyle, Tx, col, el, row, spacer, text};
use rune_theme_opencode::icons::{CIRCLE_CHECK, OUTLINE_TEXT, SHIELD, WARNING};

use super::controls::{ButtonKind, DESCRIPTION, Paint, state_button};
use super::icons::{CLOCK, MIC, WAVE};
use super::requester_icon::Icons;
use crate::desktop::i18n::{tr, tr_fill};
use crate::local_api::{Answer, Intent, Prompt, Recipient, SandboxKind, Verification};

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
/// How often a shown window checks that its first frame reached the screen.
const FRAME_CHECK: Duration = Duration::from_millis(30);

/// Where an answer goes: the consent gate's `answer(id, answer)`.
pub(crate) type Reply = Arc<dyn Fn(u64, Answer) + Send + Sync>;

/// The permission window's state.
#[derive(Default)]
pub(crate) struct Sheet {
    prompt: Option<Prompt>,
    /// When the guard last started: the current ask's first frame on
    /// screen, or the latest key pressed while it shows.
    shown_at: Option<Instant>,
    /// Frames the window had presented when the current ask asked to show,
    /// while its first frame since is still to come.
    presents_before: Option<u64>,
    /// The guard has passed: the allows work.
    armed: bool,
    reply: Option<Reply>,
    overlay: Option<rune::OverlayHandle<Sheet>>,
    /// Decodes the requester's icon off the window's thread.
    icons: Icons,
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
        self.armed = false;
        // The guard starts once the window is on screen, not when asked to
        // show: a slow first frame mustn't use it up unseen. Headless (no
        // overlay), it starts now.
        match &self.overlay {
            Some(overlay) => {
                self.shown_at = None;
                self.presents_before = Some(overlay.surface_stats().totals.presents);
                tx.wake_after(FRAME_CHECK);
            }
            None => self.restart_guard(Instant::now(), tx),
        }
        if let Some(overlay) = &self.overlay {
            #[cfg(target_os = "macos")]
            place(overlay);
            overlay.show();
            overlay.request_keys();
            #[cfg(not(target_os = "macos"))]
            if let Some(reply) = &self.reply {
                when_unshown(
                    overlay.clone(),
                    reply.clone(),
                    self.prompt.as_ref().map_or(0, |p| p.id),
                );
            }
        }
    }

    /// Hides the window: the ask was answered, timed out, or withdrawn.
    fn close(&mut self) {
        self.prompt = None;
        self.shown_at = None;
        self.presents_before = None;
        self.armed = false;
        if let Some(overlay) = &self.overlay {
            overlay.hide();
        }
    }

    /// Disables both allows for another [`GUARD`] from `now`.
    fn restart_guard(&mut self, now: Instant, tx: &mut Tx) {
        self.shown_at = Some(now);
        self.armed = false;
        tx.wake_after(GUARD);
    }

    /// Starts the guard at the ask's first frame on screen; true when it
    /// started.
    fn first_frame(&mut self, now: Instant, tx: &mut Tx) -> bool {
        let (Some(before), Some(overlay)) = (self.presents_before, &self.overlay) else {
            return false;
        };
        if overlay.surface_stats().totals.presents <= before {
            tx.wake_after(FRAME_CHECK);
            return false;
        }
        self.presents_before = None;
        self.restart_guard(now, tx);
        true
    }

    /// A key pressed while the window shows: whatever it does, the allows
    /// wait out a fresh guard, so typing meant for another app can't reach
    /// one. Never consumes the key.
    fn key_pressed(&mut self, tx: &mut Tx) -> bool {
        if self.prompt.is_some() && self.presents_before.is_none() {
            self.restart_guard(Instant::now(), tx);
        }
        false
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

fn poll(sheet: &mut Sheet, _: f64, tx: &mut Tx) -> bool {
    let now = Instant::now();
    sheet.first_frame(now, tx) | sheet.arm(now)
}

/// How long after showing Linux and Windows check that the window opened.
#[cfg(not(target_os = "macos"))]
const SHOW_CHECK: Duration = Duration::from_secs(1);

/// Linux and Windows: answers the ask `id` unavailable when its window could
/// not open (Wayland without layer-shell, or a failed show), so the asker
/// hears at once instead of after a minute nobody could see. The gate
/// ignores it once the ask has ended.
#[cfg(not(target_os = "macos"))]
fn when_unshown(overlay: rune::OverlayHandle<Sheet>, reply: Reply, id: u64) {
    let _ = std::thread::Builder::new()
        .name("hex-consent-check".into())
        .spawn(move || {
            std::thread::sleep(SHOW_CHECK);
            if overlay.availability() != rune::Availability::Unavailable {
                return;
            }
            let failure = overlay.failure();
            tracing::warn!(
                failure = failure.as_deref().unwrap_or("no reason given"),
                "Hex's permission window could not open; the ask is unavailable"
            );
            reply(id, Answer::Unavailable);
        });
}

/// Adds the permission window to the app, hidden, and returns what the
/// consent gate calls as each ask starts (`Some`) and ends (`None`); it may
/// be called from any thread.
pub(crate) fn install<S: 'static>(
    app: &mut rune::App<S>,
    reply: Reply,
) -> impl Fn(Option<Prompt>) + Send + Sync + 'static {
    use rune::{DisplayTarget, KeyPolicy, OverlayInput, OverlayOptions, OverlaySpec};
    let spec = OverlaySpec::new("Hex permission request", SIZE, view)
        .options(OverlayOptions {
            // macOS places it for real each time it shows (see [`place`]).
            // Elsewhere it spans the screen under the pointer, the dialog
            // centred in it; only the dialog takes the pointer.
            #[cfg(not(target_os = "macos"))]
            placement: Some(rune::OverlayPlacement::FullScreen),
            display: DisplayTarget::UnderPointer,
            input: OverlayInput::Regions,
            keys: KeyPolicy::OnDemand,
            ..Default::default()
        })
        .release_gpu_when_hidden()
        .setup(|sheet, waker| sheet.icons.set_cache(rune::ImageCache::new(waker)))
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
fn requester(p: Paint, icons: &Icons, recipient: &Recipient) -> El {
    let c = &p.k.c;
    let (kind, proof) = match recipient {
        Recipient::Native { verification, .. } => (tr("App"), verified(verification)),
        Recipient::Web { .. } => (
            tr("Website"),
            Proof {
                line: tr("Address reported by your browser").to_owned(),
                tone: ProofTone::Reported,
            },
        ),
    };
    // A native app leads with the name Hex derived (or its program's file
    // name), its identifier under it, each once; a website is named by its
    // address alone.
    let name = recipient.display_name().to_owned();
    let identity = recipient.identity_line().map(str::to_owned);
    let mut said = format!("{kind}: {name}");
    // A name only a desktop entry or an unsigned file gave is marked, as a
    // page's own name is.
    let unverified = recipient.name_unverified().then(|| tr("Name not verified"));
    if let Some(note) = unverified {
        said.push_str(&format!(" ({note})"));
    }
    if let Some(identity) = &identity {
        said.push_str(&format!(", {identity}"));
    }
    said.push_str(&format!(". {}.", proof.line));
    let (proof_icon, proof_color): (&'static Icon, _) = match proof.tone {
        ProofTone::Verified => (&CIRCLE_CHECK, c.success.icon),
        ProofTone::Unsigned => (&WARNING, c.warning.icon),
        ProofTone::Reported => (&SHIELD, c.icon_muted),
    };
    let mut copy = col()
        .key("requester-copy")
        .grow(1.0)
        .shrink()
        .gap(1.0)
        .child(
            text(kind)
                .key("requester-kind")
                .font(p.k.ty.caption())
                .color(p.muted())
                .a11y_hidden(),
        )
        .child(
            text(name)
                .key("recipient-value")
                .font(p.k.ty.body_medium)
                .color(p.text())
                .wrap()
                .a11y_hidden(),
        );
    if let Some(note) = unverified {
        copy = copy.child(
            text(note)
                .key("name-unverified")
                .font(DESCRIPTION)
                .color(p.faint())
                .wrap()
                .a11y_hidden(),
        );
    }
    if let Some(identity) = identity {
        copy = copy.child(
            text(identity)
                .key("recipient-identifier")
                .font(DESCRIPTION)
                .color(p.muted())
                .wrap()
                .a11y_hidden(),
        );
    }
    copy = copy.child(
        row()
            .key("proof")
            .gap(5.0)
            .pt(4.0)
            .items_start()
            .child(
                rune::icon(proof_icon, 14.0)
                    .key("proof-icon")
                    .color(proof_color)
                    .mt(1.0)
                    .a11y_hidden(),
            )
            .child(
                text(proof.line)
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
        .child(super::requester_icon::mark(p, icons, recipient, 36.0))
        .child(copy)
}

/// How the OS vouched for a requester, as the identity card's last line.
pub(super) struct Proof {
    pub(super) line: String,
    pub(super) tone: ProofTone,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProofTone {
    /// The OS verified it: a signature, or a sandbox's app id.
    Verified,
    /// Only the program's path, with no signature behind it.
    Unsigned,
    /// What the browser reported: a website's address.
    Reported,
}

/// The line for each way the OS can verify a native app. The app id or path
/// is already the card's title or the line under it, so it isn't repeated.
pub(super) fn verified(verification: &Verification) -> Proof {
    let (line, tone) = match verification {
        Verification::MacSignature { team: Some(team) } => (
            tr_fill("Code signature verified · team {}", team),
            ProofTone::Verified,
        ),
        Verification::MacSignature { team: None } => (
            tr("Code signature verified · no team").to_owned(),
            ProofTone::Verified,
        ),
        Verification::WindowsSignature { publisher } => {
            (tr_fill("Signed by {}", publisher), ProofTone::Verified)
        }
        Verification::Sandbox {
            kind: SandboxKind::Flatpak,
            ..
        } => (tr("Flatpak app").to_owned(), ProofTone::Verified),
        Verification::Sandbox {
            kind: SandboxKind::Snap,
            ..
        } => (tr("Snap app").to_owned(), ProofTone::Verified),
        Verification::Executable { .. } => (tr("Not signed").to_owned(), ProofTone::Unsigned),
    };
    Proof { line, tone }
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
                // Disabled during the guard yet still in the Tab order, so a
                // key that restarts the guard keeps the focus where it is.
                .child(
                    state_button(
                        p,
                        "browser-connect-always",
                        tr("Always Allow"),
                        ButtonKind::Secondary,
                        !armed,
                        |sheet: &mut Sheet, _| answer(sheet, Answer::Always),
                    )
                    .focusable(true),
                )
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
                .child(
                    state_button(
                        p,
                        "browser-connect-allow",
                        tr("Allow Once"),
                        ButtonKind::Primary,
                        !armed,
                        |sheet: &mut Sheet, _| answer(sheet, Answer::Once),
                    )
                    .focusable(true),
                ),
        )
        .child(
            text(tr(
                "Always Allow lasts until you revoke it in Hex › Connected apps.",
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
    root.center()
        .child(dialog(p, &sheet.icons, prompt, sheet.armed))
}

fn dialog(p: Paint, icons: &Icons, prompt: &Prompt, armed: bool) -> El {
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
        .child(requester(p, icons, &prompt.recipient))
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
    // Every other key restarts the guard before it moves focus or presses,
    // except a Space once the allows have waited out a quiet guard.
    panel
        .on_key(|sheet: &mut Sheet, event: &rune::KeyEvent, tx: &mut Tx| {
            let space = matches!(&event.key, rune::Key::Text(typed) if typed == " ");
            match event.key {
                rune::Key::Escape | rune::Key::Enter => false,
                _ if space && sheet.armed => false,
                _ => sheet.key_pressed(tx),
            }
        })
        .modal(false, |sheet: &mut Sheet| answer(sheet, Answer::Deny))
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
                icon: None,
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
                verification: Verification::MacSignature {
                    team: Some("ABCDE12345".into()),
                },
                name: None,
                icon: None,
            },
        }
    }

    /// A recording ask from a signed app whose bundle gave a name and icon.
    fn named_app_ask() -> Prompt {
        Prompt {
            id: 11,
            intent: Intent::Record,
            recipient: Recipient::Native {
                identifier: "com.example.fieldnotes".into(),
                cdhash: "fixture-digest".into(),
                verification: Verification::MacSignature {
                    team: Some("K7Z4Q2M8A1".into()),
                },
                name: Some("Field Notes".into()),
                icon: Some(super::super::requester_icon::fixtures::app()),
            },
        }
    }

    /// A transcription ask from a website that sent its favicon.
    fn site_ask() -> Prompt {
        Prompt {
            id: 12,
            intent: Intent::Transcribe,
            recipient: Recipient::Web {
                origin: "https://notes.example".into(),
                client_name: Some("Field Notes".into()),
                icon: Some(super::super::requester_icon::fixtures::favicon()),
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
        // In the Tab order, but inert.
        assert!(focusable(&rt, "browser-connect-allow"));
        assert!(focusable(&rt, "browser-connect-always"));
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

    /// Typing that lands in the window can't answer Allow: Tab then Space
    /// right after the guard, or Shift-Tab to Always Allow, restarts the
    /// guard first. After a pause with no key, Space presses the focused
    /// allow, so the keyboard can still answer.
    #[test]
    fn keys_restart_the_guard() {
        for shift in [false, true] {
            let (mut rt, replies) = shown(record_ask(), true, false);
            assert!(rt.state.armed);
            rt.key(rune::KeyEvent {
                key: rune::Key::Tab,
                mods: rune::Mods {
                    shift,
                    ..Default::default()
                },
            });
            rt.frame(0.0);
            assert!(!rt.state.armed, "Tab restarted the guard");
            key(&mut rt, rune::Key::Text(" ".into()));
            assert!(replies.lock().unwrap().is_empty(), "Space granted nothing");
            assert!(!rt.state.armed);
            // A quiet guard later, Space presses what Tab reached.
            rt.update(|sheet, _| {
                sheet.shown_at = Some(Instant::now() - GUARD);
                assert!(sheet.arm(Instant::now()));
            });
            rt.settle_all();
            rt.frame(0.0);
            key(&mut rt, rune::Key::Text(" ".into()));
            let answered = if shift { Answer::Always } else { Answer::Once };
            assert_eq!(*replies.lock().unwrap(), [(9, answered)]);
        }
    }

    /// The guard starts when the window's first frame reaches the screen,
    /// not when it is asked to show, so a slow first frame can't use it up.
    #[test]
    fn the_guard_waits_for_the_first_frame() {
        let mut sheet = Sheet::new(Arc::new(|_, _| {}));
        sheet.prompt = Some(record_ask());
        sheet.presents_before = Some(3);
        // No overlay to report frames: nothing starts it here.
        let mut tx = Tx::default();
        assert!(!sheet.first_frame(Instant::now(), &mut tx));
        assert!(sheet.shown_at.is_none());
        assert!(!sheet.arm(Instant::now() + GUARD * 5));
        // A key before the first frame doesn't arm or start it either.
        sheet.key_pressed(&mut tx);
        assert!(sheet.shown_at.is_none());
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
        // The browser reports the address; Hex can't verify it.
        assert!(copy.iter().any(|s| s == "Address reported by your browser"));
    }

    #[test]
    fn always_allow_is_revoked_in_connected_apps() {
        for prompt in [ask(), record_ask()] {
            let (rt, _) = shown(prompt, true, false);
            assert!(
                copy(&rt).iter().any(
                    |s| s == "Always Allow lasts until you revoke it in Hex › Connected apps."
                )
            );
        }
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

    /// Whether the identity card holds an element keyed `key`.
    fn card_has(rt: &rune::Runtime<Sheet>, key: &str) -> bool {
        rt.elements()
            .into_iter()
            .any(|e| e.key.as_deref() == Some(key) && e.path.contains("recipient"))
    }

    /// A named app leads with the name its bundle gave, its identifier under
    /// it, and shows its icon in place of the glyph; a website keeps its
    /// address first, with its favicon on the tile.
    #[test]
    fn the_requester_is_named_first_with_its_icon() {
        let (rt, _) = shown(named_app_ask(), true, false);
        let words = copy(&rt);
        let name = words.iter().position(|s| s == "Field Notes").unwrap();
        let identifier = words
            .iter()
            .position(|s| s == "com.example.fieldnotes")
            .unwrap();
        assert!(name < identifier);
        assert!(
            words
                .iter()
                .any(|s| s == "Code signature verified · team K7Z4Q2M8A1")
        );
        let tile = rt
            .elements()
            .into_iter()
            .find(|e| e.key.as_deref() == Some("tile"))
            .unwrap();
        assert!(!card_has(&rt, "glyph"), "the icon replaces the glyph");
        assert!(tile.rect[2] > 0.0);

        let (rt, _) = shown(site_ask(), true, false);
        let words = copy(&rt);
        assert_eq!(
            words
                .iter()
                .find(|s| s.starts_with("https://"))
                .map(String::as_str),
            Some("https://notes.example")
        );
        assert!(card_has(&rt, "favicon"));
        // Without an icon, the glyph stays.
        let (rt, _) = shown(ask(), true, false);
        assert!(!card_has(&rt, "favicon"));
        assert!(card_has(&rt, "glyph"));
    }

    /// A program known only by its path is titled by its file name, with
    /// the path once under it and "Not signed" as its proof; a derived name
    /// takes the title, and the path still shows once.
    #[test]
    fn a_programs_path_shows_once() {
        let program = |name: Option<&str>| Prompt {
            id: 13,
            intent: Intent::Record,
            recipient: Recipient::Native {
                identifier: "/opt/notes/bin/notes".into(),
                cdhash: String::new(),
                verification: Verification::Executable {
                    path: "/opt/notes/bin/notes".into(),
                },
                name: name.map(str::to_owned),
                icon: None,
            },
        };
        for (name, title) in [(None, "notes"), (Some("Field Notes"), "Field Notes")] {
            let (rt, _) = shown(program(name), true, false);
            let words = copy(&rt);
            let mentions = words.iter().filter(|s| s.contains("/opt/notes/bin/notes"));
            assert_eq!(mentions.count(), 1, "{words:?}");
            let at = |text: &str| words.iter().position(|s| s == text).unwrap();
            assert!(at(title) < at("/opt/notes/bin/notes"));
            assert!(at("/opt/notes/bin/notes") < at("Not signed"));
            // A desktop entry's name is marked; a bare file name isn't.
            assert_eq!(card_has(&rt, "name-unverified"), name.is_some());
            if name.is_some() {
                assert!(at(title) < at("Name not verified"));
            }
        }
    }

    /// A signed app's name, and a sandbox's (from its desktop entry), are
    /// marked only when nothing the OS signed backs them.
    #[test]
    fn only_a_name_without_a_signature_is_marked() {
        use crate::local_api::SandboxKind;
        let (rt, _) = shown(named_app_ask(), true, false);
        assert!(!card_has(&rt, "name-unverified"));
        for (verification, marked) in [
            (
                Verification::WindowsSignature {
                    publisher: "Example Software Ltd".into(),
                },
                false,
            ),
            (
                Verification::Sandbox {
                    kind: SandboxKind::Flatpak,
                    app_id: "org.example.Notes".into(),
                },
                true,
            ),
        ] {
            let mut prompt = named_app_ask();
            if let Recipient::Native {
                verification: slot,
                identifier,
                ..
            } = &mut prompt.recipient
            {
                *slot = verification;
                *identifier = "org.example.Notes".into();
            }
            let (rt, _) = shown(prompt, true, false);
            assert_eq!(card_has(&rt, "name-unverified"), marked);
            let words = copy(&rt);
            assert_eq!(
                words
                    .iter()
                    .filter(|s| s.contains("org.example.Notes"))
                    .count(),
                1
            );
        }
    }

    /// Each way the OS verified an app reads as what it is, and a program
    /// with only a path says it is not signed.
    #[test]
    fn each_verification_has_its_own_line() {
        use crate::local_api::SandboxKind;
        for (verification, line, tone) in [
            (
                Verification::WindowsSignature {
                    publisher: "Example Software Ltd".into(),
                },
                "Signed by Example Software Ltd",
                ProofTone::Verified,
            ),
            (
                Verification::Sandbox {
                    kind: SandboxKind::Flatpak,
                    app_id: "org.example.Notes".into(),
                },
                "Flatpak app",
                ProofTone::Verified,
            ),
            (
                Verification::Sandbox {
                    kind: SandboxKind::Snap,
                    app_id: "notes".into(),
                },
                "Snap app",
                ProofTone::Verified,
            ),
            (
                Verification::Executable {
                    path: "/usr/bin/notes".into(),
                },
                "Not signed",
                ProofTone::Unsigned,
            ),
            (
                Verification::MacSignature { team: None },
                "Code signature verified · no team",
                ProofTone::Verified,
            ),
        ] {
            let proof = verified(&verification);
            assert_eq!((proof.line.as_str(), proof.tone), (line, tone));
        }
    }

    /// Renders the permission window to `HEX_CONSENT_SNAPS/*.png`
    /// over a plain desktop grey, to look at.
    #[test]
    #[ignore = "set HEX_CONSENT_SNAPS to a folder; needs a GPU"]
    fn snapshots() {
        let folder = std::path::PathBuf::from(std::env::var("HEX_CONSENT_SNAPS").unwrap());
        let gpu = rune::render::Gpu::try_new(rune::render::Gpu::instance(), None).unwrap();
        for (name, prompt, armed, dark) in [
            ("standalone-record-light", record_ask(), true, false),
            ("standalone-record-dark", record_ask(), true, true),
            ("standalone-transcribe-light", ask(), true, false),
            ("standalone-transcribe-dark", ask(), true, true),
            ("standalone-guard-light", record_ask(), false, false),
            ("conn-dialog-app-light", named_app_ask(), true, false),
            ("conn-dialog-app-dark", named_app_ask(), true, true),
            ("conn-dialog-site-light", site_ask(), true, false),
            ("conn-dialog-site-dark", site_ask(), true, true),
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
            let path = folder.join(format!("{name}.png"));
            rune::render::write_png(&path, w, h, &target.read(&gpu.device)).unwrap();
        }
    }
}
