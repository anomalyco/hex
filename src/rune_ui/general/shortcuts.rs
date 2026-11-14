//! A shortcut's row in General and the other panes' shortcut cards: the
//! recorder, its Reset action, and what the desktop's shortcuts portal
//! granted.

use super::*;
use crate::app_settings::HotkeyBinding;

/// A shortcut's row with what a shortcuts portal granted, and the note when
/// the desktop kept it.
pub(in crate::rune_ui) fn shortcut_rows(
    ui: &Ui,
    p: Paint,
    key: &str,
    glyph: &'static rune::Icon,
    title: &str,
    target: ShortcutTarget,
    purpose: Option<&str>,
) -> Vec<CardRow> {
    let mut control = row()
        .key(format!("{key}-control"))
        .gap(4.0)
        .center_y()
        .child(keys_button(ui, p, target));
    // Paste last dictation's default, offered while the shortcut is
    // something else or off. Cancel's default, Escape, can't be recorded
    // (Escape ends the recorder), so Reset is how it comes back.
    let mut default_note = None;
    let default = match target {
        ShortcutTarget::PasteLast => Some(HotkeyBinding::paste_last_default()),
        ShortcutTarget::Cancel => Some(HotkeyBinding::cancel_default()),
        _ => None,
    };
    if let Some(default) = default {
        let current = s::binding(&ui.settings, target);
        if current.is_none() {
            default_note = Some((
                tr_fill("Default shortcut: {}", &default.keycaps().join("+")),
                p.muted(),
            ));
        }
        if current != Some(&default) {
            let reset = quiet_button(
                p,
                &format!("restore-{key}"),
                tr("Reset"),
                tr("Restore default shortcut"),
                move |ui: &mut Ui| s::restore_default(ui, target, default.clone()),
            );
            control = row().gap(6.0).center_y().child(reset).child(control);
        }
    }
    // While an optional shortcut records, plain ⌫ turns it off.
    let listening =
        matches!(ui.capture, Capture::Listening { target: active, .. } if active == target);
    let erase_hint = (listening && s::can_turn_off(target)).then(|| {
        let erase = if cfg!(target_os = "macos") {
            "⌫"
        } else {
            "Backspace"
        };
        (tr_fill("Press {} to turn it off", erase), p.muted())
    });
    let mut rows: Vec<CardRow> = vec![
        icon_described_row(
            p,
            key,
            glyph,
            title,
            erase_hint
                .or(default_note)
                .or_else(|| s::granted(ui, p, target))
                .or_else(|| purpose.map(|purpose| (purpose.to_string(), p.muted()))),
            control,
        )
        .into(),
    ];
    rows.extend(s::refused_note(ui, p, target).map(CardRow::from));
    rows.extend(s::gnome_clash_note(ui, p, target).map(CardRow::from));
    rows
}
