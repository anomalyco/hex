//! Labs › Connected apps' card while on: Allow websites to use Hex, and
//! while that is on the websites allowed to ask, those that tried to contact
//! Hex without being allowed (Allow or Block each), and those blocked; then
//! every app and website connected with a saved grant, each with Revoke.

use rune::{El, col, row, text};
use rune_kit::{ControlSize, Switch};

use super::super::Ui;
use super::super::controls::{ButtonKind, DESCRIPTION, Paint, button};
use crate::desktop::i18n::{event_age, tr};
use crate::local_api::WebsiteAction;

pub(super) fn body(ui: &Ui, p: Paint) -> El {
    let websites_on = ui.settings.labs.connected_apps_websites;
    let mut card = col().key("connected-apps").gap(4.0).child(
        row()
            .key("connected-apps-websites-row")
            .gap(8.0)
            .center_y()
            .child(
                text(tr("Allow websites to use Hex"))
                    .key("label")
                    .font(DESCRIPTION)
                    .color(p.muted()),
            )
            .child(
                Switch::new(websites_on)
                    .control_size(ControlSize::Mini)
                    .label(tr("Allow websites to use Hex"))
                    .hide_label()
                    .on_toggle(|ui: &mut Ui| {
                        ui.update(|settings| {
                            settings.labs.connected_apps_websites =
                                !settings.labs.connected_apps_websites;
                        });
                    })
                    .build(p.k)
                    .key("connected-apps-websites"),
            ),
    );
    let view = ui.view.as_ref();
    if websites_on && let Some(view) = view {
        let websites = &view.websites;
        if !websites.allowed.is_empty() {
            card = card.child(label(p, "connected-allowed-label", tr("Allowed websites")));
            for (index, origin) in websites.allowed.iter().enumerate() {
                card = card.child(site(
                    p,
                    &format!("allowed{index}"),
                    origin,
                    None,
                    &[(tr("Remove"), WebsiteAction::Forget)],
                ));
            }
        }
        if !websites.tried.is_empty() {
            card = card.child(label(
                p,
                "connected-tried-label",
                tr("Websites that tried to contact Hex"),
            ));
            for (index, tried) in websites.tried.iter().enumerate() {
                card = card.child(site(
                    p,
                    &format!("tried{index}"),
                    &tried.origin,
                    Some(event_age(tried.at_ms)),
                    &[
                        (tr("Allow"), WebsiteAction::Allow),
                        (tr("Block"), WebsiteAction::Block),
                    ],
                ));
            }
        }
        if !websites.blocked.is_empty() {
            card = card.child(label(p, "connected-blocked-label", tr("Blocked websites")));
            for (index, origin) in websites.blocked.iter().enumerate() {
                card = card.child(site(
                    p,
                    &format!("blocked{index}"),
                    origin,
                    None,
                    &[(tr("Unblock"), WebsiteAction::Forget)],
                ));
            }
        }
    }
    let Some(view) = view else {
        return card;
    };
    let once: Vec<_> = view.grants.iter().filter(|grant| !grant.durable).collect();
    if view.permissions.is_empty() && once.is_empty() {
        return card;
    }
    card = card.child(label(p, "connected-label", tr("Connected")));
    for perm in &view.permissions {
        let id = perm.id;
        card = card.child(connected(
            p,
            &format!("permission-{id}"),
            format!("{} · {}", perm.recipient.label(), perm.capability.label()),
            button(
                p,
                &format!("permission-revoke-{id}"),
                tr("Revoke"),
                ButtonKind::Secondary,
                move |ui: &mut Ui, _| ui.send(super::super::Request::RevokePermission { id }),
            ),
        ));
    }
    for grant in once {
        let id = grant.id;
        card = card.child(connected(
            p,
            &format!("api-paired-{id}"),
            format!(
                "{} · {}",
                grant.recipient.label(),
                tr("Allowed one recording")
            ),
            button(
                p,
                &format!("api-revoke-{id}"),
                tr("Revoke"),
                ButtonKind::Secondary,
                move |ui: &mut Ui, _| ui.send(super::super::Request::RevokeRecordGrant { id }),
            ),
        ));
    }
    card.child(
        text(tr(
            "Revoking ends a recording it is running. It can pair again, with a new ask.",
        ))
        .key("connected-note")
        .font(DESCRIPTION)
        .color(p.muted())
        .wrap(),
    )
}

fn label(p: Paint, key: &str, words: &str) -> El {
    text(words.to_owned())
        .key(key)
        .font(DESCRIPTION)
        .color(p.muted())
        .pt(6.0)
}

/// One website in a list (`key` names its row): its origin, when it last
/// tried (for a website that tried), and what the window can do with it.
fn site(
    p: Paint,
    key: &str,
    origin: &str,
    age: Option<String>,
    actions: &[(&'static str, WebsiteAction)],
) -> El {
    let mut name = origin.to_owned();
    if let Some(age) = age {
        name = format!("{name} · {age}");
    }
    let mut row = row().key(key.to_owned()).gap(8.0).center_y().child(
        text(name)
            .key("name")
            .font(DESCRIPTION)
            .color(p.text())
            .wrap()
            .grow(1.0)
            .shrink(),
    );
    for (index, (words, action)) in actions.iter().enumerate() {
        let (origin, action) = (origin.to_owned(), *action);
        row = row.child(button(
            p,
            &format!("{key}-action{index}"),
            words,
            ButtonKind::Secondary,
            move |ui: &mut Ui, _| {
                ui.send(super::super::Request::Website {
                    origin: origin.clone(),
                    action,
                })
            },
        ));
    }
    row
}

fn connected(p: Paint, key: &str, name: String, revoke: El) -> El {
    row()
        .key(key.to_owned())
        .gap(8.0)
        .center_y()
        .child(
            text(name)
                .key("name")
                .font(DESCRIPTION)
                .color(p.text())
                .wrap()
                .grow(1.0)
                .shrink(),
        )
        .child(revoke)
}
