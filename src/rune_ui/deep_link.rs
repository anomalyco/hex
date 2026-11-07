//! Links that open the window on a page: `hex://labs`,
//! `hex://labs/<feature>` (`hex labs link <feature>` prints it), and
//! `hex://settings/<pane>` (`hex-test://` in a test build). A Labs link
//! opens the store, on Turned on with `?tab=on`, on a chip with
//! `?category=<id>`, searching with `?search=<words>`, and on a lab's group
//! chip when it names one; it never turns a lab on. Story links live
//! in `story_link`.

use rune::Tx;

use super::labs::LabsLink;
use super::story_link::{MAX_LINK, percent_decode};
use super::{Pane, Ui};

/// Where a link opens the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// The Labs store. Names are checked when the link opens, so an
    /// unknown lab or category still opens Labs.
    Labs(LabsLink),
    /// A main pane, by its link name.
    Settings(String),
}

impl LabsLink {
    fn read(name: String, url: &url::Url) -> Self {
        let mut link = Self {
            lab: (!name.is_empty()).then_some(name),
            ..Self::default()
        };
        for (key, value) in url.query_pairs() {
            match &*key {
                "tab" => link.turned_on = value.eq_ignore_ascii_case("on"),
                "category" if !value.is_empty() => {
                    link.category = Some(value.to_ascii_lowercase());
                }
                "search" if !value.trim().is_empty() => link.search = Some(value.into_owned()),
                _ => {}
            }
        }
        link
    }
}

/// The main panes a `hex://settings/<pane>` link names.
const PANES: [(&str, Pane); 5] = [
    ("general", Pane::General),
    ("output", Pane::Output),
    ("history", Pane::History),
    ("meetings", Pane::Meetings),
    ("voice-filter", Pane::VoiceFilter),
];

/// Reads `scheme://labs[/<feature>][?…]` and `scheme://settings/<pane>`. `None`
/// means it is not one of these (a story link, or not ours), for the caller
/// to read another way.
pub(crate) fn parse(link: &str, scheme: &str) -> Option<Result<Target, String>> {
    if link.len() > MAX_LINK {
        return None;
    }
    let url = url::Url::parse(link).ok()?;
    if url.scheme() != scheme {
        return None;
    }
    let host = url.host_str()?;
    if host != "labs" && host != "settings" {
        return None;
    }
    let segments: Option<Vec<String>> = url
        .path_segments()
        .into_iter()
        .flatten()
        .filter(|segment| !segment.is_empty())
        .map(percent_decode)
        .collect();
    let Some(segments) = segments else {
        return Some(Err("the link isn't valid UTF-8".into()));
    };
    let name = segments.join("/").to_ascii_lowercase();
    Some(match (host, name.is_empty()) {
        ("labs", _) => Ok(Target::Labs(LabsLink::read(name, &url))),
        (_, true) => Err("the link names no settings pane".into()),
        (_, false) => Ok(Target::Settings(name)),
    })
}

/// The link that opens Labs on `feature`'s group.
pub(crate) fn labs_link(scheme: &str, feature: crate::labs::Feature) -> String {
    format!("{scheme}://labs/{}", feature.name())
}

/// Opens the window's page for `target`; the caller brings the window forward.
pub(crate) fn open(ui: &mut Ui, target: Target, tx: &mut Tx) {
    match target {
        Target::Labs(link) => super::labs::reveal(ui, &link, tx),
        Target::Settings(name) => {
            let pane = PANES
                .iter()
                .find(|(known, _)| *known == name)
                .map(|(_, p)| *p);
            match pane {
                Some(pane) if pane.shown(ui) => {
                    super::show(ui, super::Destination::Settings, tx);
                    ui.select_pane(pane, tx);
                }
                _ => {
                    tracing::warn!(pane = %name, "a settings link named no pane this Hex shows");
                    super::show(ui, super::Destination::Settings, tx);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labs::Feature;

    fn read(link: &str) -> Option<Result<Target, String>> {
        parse(link, "hex")
    }

    fn lab(id: &str) -> Target {
        Target::Labs(LabsLink {
            lab: Some(id.into()),
            ..LabsLink::default()
        })
    }

    #[test]
    fn reads_labs_links() {
        let store = Target::Labs(LabsLink::default());
        assert_eq!(read("hex://labs"), Some(Ok(store.clone())));
        assert_eq!(read("hex://labs/"), Some(Ok(store)));
        assert_eq!(read("hex://labs/live-text"), Some(Ok(lab("live-text"))));
        assert_eq!(read("hex://labs/Live-Text/"), Some(Ok(lab("live-text"))));
        assert_eq!(
            parse("hex-test://labs/meetings", "hex-test"),
            Some(Ok(lab("meetings")))
        );
    }

    #[test]
    fn reads_the_labs_stores_tab_chip_and_search() {
        let labs = |link: LabsLink| Some(Ok(Target::Labs(link)));
        assert_eq!(
            read("hex://labs?tab=on"),
            labs(LabsLink {
                turned_on: true,
                ..LabsLink::default()
            })
        );
        assert_eq!(read("hex://labs?tab=discover"), labs(LabsLink::default()));
        assert_eq!(
            read("hex://labs?category=Meetings"),
            labs(LabsLink {
                category: Some("meetings".into()),
                ..LabsLink::default()
            })
        );
        assert_eq!(
            read("hex://labs?search=paste%20last&tab=on"),
            labs(LabsLink {
                turned_on: true,
                search: Some("paste last".into()),
                ..LabsLink::default()
            })
        );
        assert_eq!(
            read("hex://labs/live-text?category=dictation&search=live+text"),
            labs(LabsLink {
                lab: Some("live-text".into()),
                category: Some("dictation".into()),
                search: Some("live text".into()),
                ..LabsLink::default()
            })
        );
        assert_eq!(
            read("hex://labs?search=&category=&other=1"),
            labs(LabsLink::default())
        );
    }

    #[test]
    fn reads_settings_links() {
        assert_eq!(
            read("hex://settings/history"),
            Some(Ok(Target::Settings("history".into())))
        );
        assert!(matches!(read("hex://settings"), Some(Err(_))));
        assert!(matches!(read("hex://labs/%FF"), Some(Err(_))));
    }

    #[test]
    fn leaves_other_links_alone() {
        for other in [
            "hex://story/hud/processing",
            "hex-test://labs/live-text",
            "https://labs/live-text",
            "hex://elsewhere",
            "not a link",
            "",
        ] {
            assert_eq!(read(other), None, "{other}");
        }
        let long = format!("hex://labs/{}", "a".repeat(MAX_LINK));
        assert_eq!(read(&long), None);
    }

    #[test]
    fn every_feature_has_a_link_that_reads_back() {
        for feature in Feature::ALL {
            let link = labs_link("hex", feature);
            assert_eq!(read(&link), Some(Ok(lab(feature.name()))), "{link}");
        }
    }

    #[test]
    fn every_pane_name_is_distinct() {
        let mut names: Vec<_> = PANES.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), PANES.len());
    }
}
