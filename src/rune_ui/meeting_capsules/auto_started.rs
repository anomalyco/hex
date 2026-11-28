//! The capsule that says a recording started on its own while the recording
//! indicator is off: the call app's icon, clean, in the call-ended capsule's
//! icon slot, then "● Recording" over the call and its time, then a calm
//! Stop and ✕. Keeping the recording is the happy path, so nothing is loud.

use rune::{Cursor, El, el, row, text};

use super::super::controls::{CAPSULE, Paint};
use super::call_ended::{BODY_PL, BODY_PY, RING_SIZE, app_icon};
use super::{CAPSULE_INSET, Capsule, button, panel_radius, presence};
use crate::desktop::i18n::tr;
use crate::meeting::watcher::ControllerCommand;

/// The capsule's width: the icon, two short lines, Stop and ✕, with no
/// empty middle.
pub(super) const PANEL_WIDTH: f32 = 300.0;
/// The live dot before "Recording", a status dot's size.
const LIVE_DOT: f32 = 6.0;

/// The automatic-recording capsule in its window: the call app's icon and
/// the live dot, what is being recorded, then Stop and ✕. Dismissing leaves
/// the recording going; the menu bar still shows it.
pub(super) fn view(
    p: Paint,
    capsule: &Capsule,
    title: &str,
    app: &str,
    icon: Option<&rune::Pixels>,
) -> El {
    let c = &p.k.c;
    let heading = tr("Recording");
    let badge = row()
        .key("ring")
        .size(RING_SIZE, RING_SIZE)
        .center()
        .child(app_icon(p, app, icon));
    let subtitle = subtitle(title);
    let lines = rune::col()
        .key("lines")
        .grow(1.0)
        .min_w(0.0)
        .child(
            row()
                .key("title-line")
                .gap(6.0)
                .center_y()
                .child(live_dot(p))
                .child(
                    text(heading.to_owned())
                        .key("title")
                        .truncate()
                        .font(p.strong())
                        .color(p.text())
                        .a11y_hidden(),
                ),
        )
        .children((!subtitle.is_empty()).then(|| {
            text(subtitle.clone())
                .key("subtitle")
                .truncate()
                .font(p.meta())
                .color(p.faint())
        }));
    let actions = row()
        .key("actions")
        .gap(CAPSULE.button_inset)
        .center_y()
        // Keeping the recording is the happy path: Stop is a calm neutral
        // button, never the loud one.
        .child(button(p, "stop", tr("Stop"), false, |c| {
            if c.send(ControllerCommand::Stop) {
                c.close();
            }
        }))
        .child(dismiss_button(p));
    let body = row()
        .key("pill")
        .role("status")
        .label(if subtitle.is_empty() {
            heading.to_owned()
        } else {
            format!("{heading}, {subtitle}")
        })
        .w_full()
        .h_full()
        .center_y()
        .pl(BODY_PL)
        // The ✕ box sits on the capsules' 6 pt inset, like every one.
        .pr(CAPSULE.button_inset)
        .gap(12.0)
        .radius(panel_radius(p))
        .fill(c.bg_base)
        .border(p.hair, c.border_base)
        .child(badge)
        .child(lines)
        .child(actions)
        .hit()
        .cursor(Cursor::Grab)
        .on_drag(|c: &mut Capsule, drag, tx| c.drag(drag, tx));
    row()
        .key("capsule")
        .w_full()
        .h_full()
        .px(CAPSULE_INSET)
        .py(BODY_PY)
        .child(presence::anchored(capsule, body))
}

/// The ✕ that closes a capsule whose actions have no way to, in every
/// capsule's 28 pt close box. It only hides the capsule: nothing is sent to
/// the controller (#996).
/// The box rounds 8 pt, concentric with the capsule across its 6 pt inset.
pub(super) fn dismiss_button(p: Paint) -> El {
    row()
        .key("close-box")
        .size(CAPSULE.close, CAPSULE.close)
        .center()
        .radius(CAPSULE.inner(CAPSULE.button_inset))
        .hover(p.k.c.hover)
        .press(p.k.c.pressed)
        .role("button")
        .label(tr("Close"))
        .focusable(true)
        .focus_ring(2., -2., p.k.c.border_focus)
        .cursor(Cursor::Pointer)
        .no_window_drag()
        .hit()
        .on_press_tx(|c: &mut Capsule, tx| {
            c.close();
            c.stir(tx);
        })
        .child(
            rune::icon(&rune_theme_opencode::icons::CLOSE_SMALL, 16.0)
                .key("dismiss")
                .color(p.muted())
                .a11y_hidden(),
        )
}

/// The live recording dot before "Recording", on its own line rather than
/// over the app icon's edge.
fn live_dot(p: Paint) -> El {
    el().key("rec-dot")
        .size(LIVE_DOT, LIVE_DOT)
        .radius(LIVE_DOT / 2.0)
        .fill(p.danger())
        .a11y_hidden()
}

/// The call and its time as the capsule says them: "Slack Huddle · 2:30 PM"
/// for "Slack Huddle call — 2:30 PM", no dash and no "call".
fn subtitle(title: &str) -> String {
    title
        .replace(" call — ", " · ")
        .replace(" — ", " · ")
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::subtitle;

    #[test]
    fn the_subtitle_drops_the_dash_and_the_word_call() {
        assert_eq!(
            subtitle("Slack Huddle call — 2:30 PM"),
            "Slack Huddle · 2:30 PM"
        );
        assert_eq!(subtitle("Design sync — 2:30 PM"), "Design sync · 2:30 PM");
        assert_eq!(subtitle("Standup"), "Standup");
    }
}
