//! The capsule that says a recording started on its own while the recording
//! indicator is off. Its icon slot is the call-ended capsule's: the call
//! app's icon, with what the recording is doing on it. Here that is the red
//! dot of a live recording on the icon's corner; there it is the ring that
//! drains to an automatic stop.

use rune::{Cursor, El, el, row, text};

use super::super::controls::{CAPSULE, Paint};
use super::call_ended::{BODY_PL, BODY_PY, ICON_SIZE, RING_SIZE, app_icon};
use super::{CAPSULE_INSET, Capsule, REC_DOT, button, panel_radius, presence};
use crate::desktop::i18n::tr;
use crate::meeting::watcher::ControllerCommand;

/// The capsule's ground round the dot, so the dot reads as its own mark
/// where it overlaps the icon.
const KNOCKOUT: f32 = 2.0;
/// The dot and its ring.
const BADGE: f32 = REC_DOT + 2.0 * KNOCKOUT;
/// The dot's centre, in from the icon's bottom-right corner: on the corner's
/// own curve.
const BADGE_INSET: f32 = 2.0;
/// The badge's origin in the icon slot.
const BADGE_AT: f32 = (RING_SIZE + ICON_SIZE) / 2.0 - BADGE_INSET - BADGE / 2.0;

// The 26 pt icon spans 5..31 of the 36 pt slot: the 12 pt badge starts at 23
// and stays inside the slot.
const _: () = assert!(BADGE == 12.0 && BADGE_AT == 23.0 && BADGE_AT + BADGE <= RING_SIZE);

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
    let heading = tr("Recording automatically");
    let badge = row()
        .key("ring")
        .size(RING_SIZE, RING_SIZE)
        .center()
        .child(app_icon(p, app, icon))
        .child(live_dot(p));
    let lines = rune::col()
        .key("lines")
        .grow(1.0)
        .min_w(0.0)
        .child(
            text(heading.to_owned())
                .key("title")
                .truncate()
                .font(p.strong())
                .color(p.text())
                .a11y_hidden(),
        )
        .children((!title.is_empty()).then(|| {
            text(title.to_owned())
                .key("subtitle")
                .truncate()
                .font(p.meta())
                .color(p.faint())
        }));
    let actions = row()
        .key("actions")
        .gap(CAPSULE.button_inset)
        .center_y()
        .child(button(p, "stop", tr("Stop"), true, |c| {
            if c.send(ControllerCommand::Stop) {
                c.close();
            }
        }))
        .child(dismiss_button(p));
    let body = row()
        .key("pill")
        .role("status")
        .label(heading)
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

/// The recording dot on the icon's bottom-right corner, ringed by the
/// capsule's ground.
fn live_dot(p: Paint) -> El {
    row()
        .key("live")
        .absolute(BADGE_AT, BADGE_AT)
        .size(BADGE, BADGE)
        .center()
        .radius(BADGE / 2.0)
        .fill(p.k.c.bg_base)
        .a11y_hidden()
        .child(
            el().key("rec-dot")
                .size(REC_DOT, REC_DOT)
                .radius(REC_DOT / 2.0)
                .fill(p.danger()),
        )
}
