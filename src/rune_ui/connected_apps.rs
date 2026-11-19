//! The Connected apps pane (while Labs › Connected apps is on): every app
//! and website with a saved grant, each with Revoke; then Allow websites to
//! use Hex and, while it's on, the websites asking to connect (Allow or
//! Block each), and the allowed and blocked ones behind their disclosures.
//!
//! Names and icons come from what the recipient last asked with, kept with
//! its grant, so the pane shows them while it is offline. A website is
//! always named by its address.

use rune::{El, Icon, col, row, text};
use rune_theme_opencode::icons::LINK;

use super::controls::{DESCRIPTION, METRICS, Paint, card, page, toggle_row};
use super::lists::{Tone, small_button, tone_tag};
use super::requester_icon::{Icons, mark};
use super::{Request, Ui};
use crate::desktop::i18n::{event_age, tr, tr_fill};
use crate::local_api::{PermissionCapability, Recipient, WebsiteAction};

/// The pane's sidebar icon, and the Labs card's.
pub(super) static ICON: &Icon = &LINK;

/// A row's icon, in points.
const MARK: f32 = 32.0;

/// Which of the website disclosures are open; both start closed.
#[derive(Default)]
pub(super) struct Disclosures {
    allowed: bool,
    blocked: bool,
}

impl Disclosures {
    /// Both lists unfolded, for scenes.
    pub(super) fn open() -> Self {
        Self {
            allowed: true,
            blocked: true,
        }
    }
}

pub(super) fn pane(ui: &Ui, p: Paint) -> El {
    let content = page("content")
        .child(connected(ui, p))
        .children(websites(ui, p));
    super::scroll_body(p.k, "connected-apps-scroll", content)
}

/// One grant as the Connected list shows it.
struct Grant<'a> {
    key: String,
    recipient: &'a Recipient,
    capability: &'static str,
    always: bool,
    revoke: Request,
}

fn connected(ui: &Ui, p: Paint) -> El {
    let mut grants = Vec::new();
    if let Some(view) = ui.view.as_ref() {
        for permission in &view.permissions {
            grants.push(Grant {
                key: format!("permission-{}", permission.id),
                recipient: &permission.recipient,
                capability: capability(permission.capability),
                always: true,
                revoke: Request::RevokePermission { id: permission.id },
            });
        }
        for grant in view.grants.iter().filter(|grant| !grant.durable) {
            grants.push(Grant {
                key: format!("api-paired-{}", grant.id),
                recipient: &grant.recipient,
                capability: capability(PermissionCapability::MicrophoneRecording),
                always: false,
                revoke: Request::RevokeRecordGrant { id: grant.id },
            });
        }
    }
    let count = grants.len();
    let rows: Vec<El> = if grants.is_empty() {
        vec![empty(p)]
    } else {
        grants
            .into_iter()
            .map(|grant| grant_row(p, &ui.requester_icons, grant))
            .collect()
    };
    let mut section = counted(p, "connected", tr("Connected"), count, false, rows);
    if ui
        .view
        .as_ref()
        .is_some_and(|view| !view.permissions.is_empty() || view.grants.iter().any(|g| !g.durable))
    {
        section = section.child(footnote(
            "connected-note",
            tr("Revoking ends a recording it is running. It can pair again, with a new ask."),
            p,
        ));
    }
    section
}

fn capability(capability: PermissionCapability) -> &'static str {
    match capability {
        PermissionCapability::MicrophoneRecording => tr("Microphone recording"),
        PermissionCapability::TranscribeAudio => tr("Transcription"),
    }
}

/// An app or website and what it may do: its icon, its name, a line saying
/// who it is and what it may do, how long, and Revoke.
fn grant_row(p: Paint, icons: &Icons, grant: Grant) -> El {
    let recipient = grant.recipient;
    let name = recipient.display_name().to_owned();
    let who = match recipient {
        Recipient::Native {
            name: Some(_),
            identifier,
            ..
        } => identifier.clone(),
        Recipient::Native { .. } => tr("App").to_owned(),
        Recipient::Web { .. } => tr("Website").to_owned(),
    };
    let detail = format!(
        "{who} · {} · {}",
        grant.capability,
        if grant.always {
            tr("Always allowed")
        } else {
            tr("Allowed once")
        }
    );
    let revoke = grant.revoke;
    let label = tr_fill("Revoke {}", &name);
    item(
        p,
        &grant.key,
        mark(p, icons, recipient, MARK),
        &name,
        &detail,
        row().key("actions").child(
            small_button(
                p,
                &format!("{}-revoke", grant.key),
                tr("Revoke"),
                move |ui, _| ui.send(revoke.clone()),
            )
            .label(label),
        ),
    )
}

/// Nothing connected yet: what will show here, and how it gets here.
fn empty(p: Paint) -> El {
    row()
        .key("connected-empty")
        .px(METRICS.row_px)
        .py(18.0)
        .gap(14.0)
        .center_y()
        .child(super::icons::tile(p, ICON, 36.0))
        .child(
            col()
                .key("copy")
                .grow(1.0)
                .shrink()
                .gap(2.0)
                .child(
                    text(tr("No apps or websites are connected"))
                        .key("title")
                        .font(p.k.ty.ui_medium)
                        .color(p.text())
                        .wrap(),
                )
                .child(
                    text(tr(
                        "When an app or website asks to use Hex and you allow it, it shows here.",
                    ))
                    .key("hint")
                    .font(DESCRIPTION)
                    .color(p.muted())
                    .wrap(),
                ),
        )
}

/// Allow websites to use Hex, and while it's on, the website lists.
fn websites(ui: &Ui, p: Paint) -> Vec<El> {
    let on = ui.settings.labs.connected_apps_websites;
    let switch = toggle_row(
        p,
        "connected-apps-websites",
        tr("Allow websites to use Hex"),
        Some(if on {
            tr("Websites you allow can ask in Hex's permission window")
        } else {
            tr("Websites can't ask. Apps on this computer still can")
        }),
        on,
        |ui: &mut Ui| {
            ui.update(|settings| {
                settings.labs.connected_apps_websites = !settings.labs.connected_apps_websites;
            });
        },
    );
    let mut out = vec![
        col()
            .key("websites")
            .gap(METRICS.label_gap)
            .child(p.section_label(tr("Websites")).mx(METRICS.row_px))
            .child(card(p, "card", vec![switch])),
    ];
    let Some(view) = ui.view.as_ref().filter(|_| on) else {
        return out;
    };
    let websites = &view.websites;
    let icons = &ui.requester_icons;
    if !websites.tried.is_empty() {
        let rows = websites
            .tried
            .iter()
            .enumerate()
            .map(|(index, tried)| {
                site_row(
                    p,
                    icons,
                    &format!("tried{index}"),
                    &tried.origin,
                    None,
                    Some(tr_fill("Tried {}", &event_age(tried.at_ms))),
                    &[
                        (tr("Allow"), WebsiteAction::Allow),
                        (tr("Block"), WebsiteAction::Block),
                    ],
                )
            })
            .collect();
        out.push(counted(
            p,
            "asking",
            tr("Asking to connect"),
            websites.tried.len(),
            true,
            rows,
        ));
    }
    let lists = [
        (
            "allowed",
            tr("Allowed websites"),
            &websites.allowed,
            ui.connected.allowed,
            (tr("Remove"), WebsiteAction::Forget),
        ),
        (
            "blocked",
            tr("Blocked websites"),
            &websites.blocked,
            ui.connected.blocked,
            (tr("Unblock"), WebsiteAction::Forget),
        ),
    ];
    // The allowed and blocked lists fold away together, under the rest.
    let mut folded = col().key("website-lists").gap(14.0);
    let mut any = false;
    for (key, title, origins, open, action) in lists {
        if origins.is_empty() {
            continue;
        }
        let blocked = key == "blocked";
        let mut group = col()
            .key(key)
            .gap(METRICS.label_gap)
            .child(super::general::disclosure(
                p,
                &format!("{key}-toggle"),
                title,
                Some(origins.len()),
                open,
                move |ui: &mut Ui| {
                    let open = if blocked {
                        &mut ui.connected.blocked
                    } else {
                        &mut ui.connected.allowed
                    };
                    *open = !*open;
                },
            ));
        if open {
            let rows: Vec<El> = origins
                .iter()
                .enumerate()
                .map(|(index, origin)| {
                    site_row(
                        p,
                        icons,
                        &format!("{key}{index}"),
                        origin,
                        websites.icon(origin),
                        None,
                        &[action],
                    )
                })
                .collect();
            group = group.child(card(p, "card", rows));
        }
        folded = folded.child(group);
        any = true;
    }
    if any {
        out.push(folded);
    }
    out
}

/// One website in a list: its favicon (if it sent one while allowed), its
/// address, when it last tried, and what the pane can do with it.
fn site_row(
    p: Paint,
    icons: &Icons,
    key: &str,
    origin: &str,
    icon: Option<&crate::local_api::ConsentIcon>,
    detail: Option<String>,
    actions: &[(&'static str, WebsiteAction)],
) -> El {
    let recipient = Recipient::Web {
        origin: origin.to_owned(),
        client_name: None,
        icon: icon.cloned(),
    };
    let mut buttons = row().key("actions").gap(8.0);
    for (index, (words, action)) in actions.iter().enumerate() {
        let said = format!("{words} {origin}");
        let (origin, action) = (origin.to_owned(), *action);
        buttons = buttons.child(
            small_button(p, &format!("{key}-action{index}"), words, move |ui, _| {
                ui.send(Request::Website {
                    origin: origin.clone(),
                    action,
                })
            })
            .label(said),
        );
    }
    item(
        p,
        key,
        mark(p, icons, &recipient, MARK),
        origin,
        detail.as_deref().unwrap_or(""),
        buttons,
    )
}

/// A list row in a card: icon, title, a muted line under it, and its
/// actions on the right.
fn item(p: Paint, key: &str, icon: El, title: &str, detail: &str, actions: El) -> El {
    let heading = text(title.to_owned())
        .key("title")
        .font(p.k.ty.ui_medium)
        .color(p.text())
        .truncate()
        .min_w(0.0)
        .a11y_hidden();
    let mut copy = col()
        .key("copy")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .gap(1.0)
        .child(heading);
    if !detail.is_empty() {
        copy = copy.child(
            text(detail.to_owned())
                .key("detail")
                .font(DESCRIPTION)
                .color(p.muted())
                .truncate()
                .a11y_hidden(),
        );
    }
    let said = if detail.is_empty() {
        title.to_owned()
    } else {
        format!("{title}, {detail}")
    };
    row()
        .key(key.to_owned())
        .role("group")
        .label(said)
        .min_h(METRICS.row_h_described)
        .px(METRICS.row_px)
        .py(10.0)
        .gap(12.0)
        .center_y()
        .child(icon)
        .child(copy)
        .child(actions)
}

/// A titled card with its count after the label, as Labs counts its
/// groups; `asking` makes the count an accent tag, since those websites
/// wait on the person.
fn counted(p: Paint, key: &str, title: &str, count: usize, asking: bool, rows: Vec<El>) -> El {
    let count = (count > 0).then(|| {
        if asking {
            tone_tag(p, "count", &count.to_string(), Tone::Accent)
        } else {
            text(count.to_string())
                .key("count")
                .font(p.k.ty.ui_medium.lh(16.0))
                .color(p.faint())
                .a11y_hidden()
        }
    });
    col()
        .key(key)
        .gap(METRICS.label_gap)
        .child(
            row()
                .key("header-row")
                .h(16.0)
                .mx(METRICS.row_px)
                .gap(6.0)
                .center_y()
                .child(p.section_label(title))
                .children(count),
        )
        .child(card(p, "card", rows))
}

fn footnote(key: &str, words: &str, p: Paint) -> El {
    text(words.to_owned())
        .key(key)
        .font(DESCRIPTION)
        .color(p.muted())
        .wrap()
        .mx(METRICS.row_px)
}

#[cfg(test)]
mod tests {
    use rune::Runtime;

    use super::super::{Request, Service, Ui, offscreen, poll, view};

    /// `name`'s scene, rendered tall enough to show the whole pane.
    fn scene(name: &str, _live: bool, size: (f32, f32)) -> Runtime<Ui> {
        let mut state = Ui::with(
            crate::app_settings::AppSettings::default(),
            Service::Fixture(Vec::new()),
        );
        let (_, apply) = super::super::SCENES
            .iter()
            .find(|(scene, _)| *scene == name)
            .unwrap_or_else(|| panic!("no scene {name}"));
        apply(&mut state);
        let mut rt = offscreen::runtime(
            state,
            view,
            size,
            rune::Env {
                system_dark: false,
                scale: 2.0,
            },
            rune::text::TextSystem::new(rune_theme_opencode::FONTS, rune_theme_opencode::FAMILIES),
        );
        rt.poll = Some(poll);
        settle(&mut rt);
        rt
    }

    fn settle(rt: &mut Runtime<Ui>) {
        for _ in 0..30 {
            rt.frame(1.0 / 30.0);
        }
        rt.settle_all();
        rt.frame(0.0);
    }

    fn texts(rt: &Runtime<Ui>) -> Vec<String> {
        rt.elements().into_iter().filter_map(|e| e.text).collect()
    }

    fn tap(rt: &mut Runtime<Ui>, key: &str) {
        let (x, y) = rt.find(key).unwrap_or_else(|| panic!("no {key}"));
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(0.0);
    }

    fn sent(rt: &Runtime<Ui>) -> &[Request] {
        match &rt.state.service {
            Service::Fixture(sent) => sent,
            _ => panic!("fixtures record requests"),
        }
    }

    #[test]
    fn connected_apps_lists_who_is_connected_and_who_is_asking() {
        use crate::local_api::WebsiteAction;
        let size = (980.0, 2400.0);
        let rt = scene("connected-apps", true, size);
        let copy = texts(&rt);
        for line in [
            "Connected",
            "Field Notes",
            "com.example.fieldnotes · Microphone recording · Always allowed",
            "https://notes.example",
            "Website · Transcription · Always allowed",
            "App · Microphone recording · Allowed once",
            "Allow websites to use Hex",
            "Asking to connect",
            "Tried 4m ago",
            "Allowed websites",
            "Blocked websites",
        ] {
            assert!(
                copy.iter().any(|text| text == line),
                "{line:?} missing: {copy:?}"
            );
        }
        // The allowed and blocked lists start folded.
        assert!(rt.find("allowed0-action0").is_none());
        assert!(rt.find("blocked0-action0").is_none());
        for (key, origin, action) in [
            (
                "tried0-action0",
                "https://new-tool.example",
                WebsiteAction::Allow,
            ),
            (
                "tried0-action1",
                "https://new-tool.example",
                WebsiteAction::Block,
            ),
            (
                "tried1-action1",
                "https://meeting-notes.example",
                WebsiteAction::Block,
            ),
        ] {
            let mut rt = scene("connected-apps", true, size);
            tap(&mut rt, key);
            assert!(
                matches!(sent(&rt), [Request::Website { origin: sent, action: a }] if sent == origin && *a == action),
                "{key}"
            );
        }
        let mut rt = scene("connected-apps", true, size);
        tap(&mut rt, "permission-3-revoke");
        assert!(matches!(sent(&rt), [Request::RevokePermission { id: 3 }]));
        let mut rt = scene("connected-apps", true, size);
        tap(&mut rt, "api-paired-1-revoke");
        assert!(matches!(sent(&rt), [Request::RevokeRecordGrant { id: 1 }]));
    }

    #[test]
    fn connected_apps_unfolds_its_allowed_and_blocked_websites() {
        use crate::local_api::WebsiteAction;
        let size = (980.0, 2400.0);
        for (toggle, key, origin) in [
            (
                "allowed-toggle",
                "allowed0-action0",
                "https://notes.example",
            ),
            ("blocked-toggle", "blocked0-action0", "https://ads.example"),
        ] {
            let mut rt = scene("connected-apps", true, size);
            tap(&mut rt, toggle);
            settle(&mut rt);
            tap(&mut rt, key);
            assert!(
                matches!(sent(&rt), [Request::Website { origin: sent, action: WebsiteAction::Forget }] if sent == origin),
                "{key}"
            );
        }
    }

    #[test]
    fn connected_apps_hides_its_website_lists_while_websites_are_off() {
        let rt = scene("connected-apps-websites-off", true, (980.0, 2400.0));
        let copy = texts(&rt);
        assert!(copy.iter().any(|text| text == "Allow websites to use Hex"));
        assert!(
            copy.iter()
                .any(|text| text == "Websites can't ask. Apps on this computer still can")
        );
        for hidden in ["Asking to connect", "Allowed websites", "Blocked websites"] {
            assert!(!copy.iter().any(|text| text == hidden), "{hidden:?} shown");
        }
        assert!(
            copy.iter().any(|text| text == "Field Notes"),
            "grants stay listed"
        );
    }

    #[test]
    fn connected_apps_says_when_nothing_is_connected() {
        let rt = scene("connected-apps-empty", true, (980.0, 1200.0));
        let copy = texts(&rt);
        assert!(
            copy.iter()
                .any(|text| text == "No apps or websites are connected")
        );
        assert!(!copy.iter().any(|text| text == "Asking to connect"));
        assert!(rt.find("connected-note").is_none());
    }

    /// The pane joins the sidebar while the lab is on, and the lab's card keeps
    /// only its switch and summary.
    #[test]
    fn connected_apps_is_a_pane_while_its_lab_is_on() {
        let rt = scene("mac-labs-connected-apps", true, (980.0, 1200.0));
        assert!(rt.find("nav-connected-apps").is_some());
        let copy = texts(&rt);
        assert!(!copy.iter().any(|text| text == "Allow websites to use Hex"));
        assert!(
            copy.iter().any(|text| text
                == "Let apps and websites ask to use Hex's transcription and microphone")
        );
        let mut rt = scene("mac-labs-connected-apps", true, (980.0, 1200.0));
        rt.update(|ui, _| ui.settings.labs.connected_apps = false);
        settle(&mut rt);
        assert!(rt.find("nav-connected-apps").is_none());
    }
}
