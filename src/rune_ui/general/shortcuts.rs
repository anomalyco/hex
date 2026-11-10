//! A shortcut's row in General and the other panes' shortcut cards: the
//! recorder, its Clear and Restore default actions, and what the desktop's
//! shortcuts portal granted.

use super::*;

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
    // The optional shortcuts can be turned off.
    let second = target == ShortcutTarget::SecondDictation;
    #[cfg(target_os = "linux")]
    let typed_text = target == ShortcutTarget::TypedText;
    #[cfg(not(target_os = "linux"))]
    let typed_text = false;
    #[cfg(any(target_os = "windows", target_os = "linux"))]
    let terminal = target == ShortcutTarget::TerminalPaste;
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    let terminal = false;
    let optional = second
        || typed_text
        || terminal
        || matches!(
            target,
            ShortcutTarget::PasteLast | ShortcutTarget::RawDictation
        );
    if optional && s::binding(&ui.settings, target).is_some() {
        control = control.child(icon_button(
            p,
            &format!("clear-{key}"),
            &rune_theme_opencode::icons::TRASH,
            tr("Clear shortcut"),
            move |ui: &mut Ui| {
                ui.update(|settings| match target {
                    ShortcutTarget::RawDictation => settings.raw_dictation_hotkey = None,
                    #[cfg(target_os = "linux")]
                    ShortcutTarget::TypedText => settings.typed_text_hotkey = None,
                    _ if second => settings.second_dictation_hotkey = None,
                    _ if terminal => settings.terminal_paste_hotkey = None,
                    _ => settings.paste_last_hotkey = None,
                });
            },
        ));
    }
    // Labs › Restore shortcut default: Paste last dictation's default,
    // offered while the shortcut is something else or off. Cancel's
    // default, Escape, can't be recorded (Escape ends the recorder), so its
    // restore is always offered.
    let mut default_note = None;
    let default = match target {
        ShortcutTarget::PasteLast
            if crate::labs::Feature::RestoreShortcutDefault.on(&ui.settings) =>
        {
            Some(crate::app_settings::HotkeyBinding::paste_last_default())
        }
        ShortcutTarget::Cancel => Some(crate::app_settings::HotkeyBinding::cancel_default()),
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
            control = control.child(icon_button(
                p,
                &format!("restore-{key}"),
                &REFRESH,
                tr("Restore default shortcut"),
                move |ui: &mut Ui| s::restore_default(ui, target, default.clone()),
            ));
        }
    }
    let mut rows: Vec<CardRow> = vec![
        icon_described_row(
            p,
            key,
            glyph,
            title,
            default_note
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
