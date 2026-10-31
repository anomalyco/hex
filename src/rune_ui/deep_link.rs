//! Links that open the window on a page: `hex://labs`,
//! `hex://labs/<feature>` (`hex labs link <feature>` prints it), and
//! `hex://settings/<pane>` (`hex-test://` in a test build). A Labs link
//! scrolls to the feature's row and pulses it; it never turns the feature on.
//! Story links live in `story_link`.

use rune::Tx;

use super::story_link::{MAX_LINK, percent_decode};
use super::{Pane, Ui};

/// Where a link opens the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    /// The Labs page, at a feature's row when it names one. The name is
    /// checked when the link opens, so an unknown one still opens Labs.
    Labs(Option<String>),
    /// A main pane, by its link name.
    Settings(String),
}

/// The main panes a `hex://settings/<pane>` link names.
const PANES: [(&str, Pane); 5] = [
    ("general", Pane::General),
    ("output", Pane::Output),
    ("history", Pane::History),
    ("meetings", Pane::Meetings),
    ("voice-filter", Pane::VoiceFilter),
];

/// Reads `scheme://labs[/<feature>]` and `scheme://settings/<pane>`. `None`
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
        ("labs", true) => Ok(Target::Labs(None)),
        ("labs", false) => Ok(Target::Labs(Some(name))),
        (_, true) => Err("the link names no settings pane".into()),
        (_, false) => Ok(Target::Settings(name)),
    })
}

/// The link that opens Labs at `feature`'s row.
pub(crate) fn labs_link(scheme: &str, feature: crate::labs::Feature) -> String {
    format!("{scheme}://labs/{}", feature.name())
}

/// Opens the window's page for `target`; the caller brings the window forward.
pub(crate) fn open(ui: &mut Ui, target: Target, tx: &mut Tx) {
    match target {
        Target::Labs(name) => super::labs::reveal(ui, name.as_deref(), tx),
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

    #[test]
    fn reads_labs_links() {
        assert_eq!(read("hex://labs"), Some(Ok(Target::Labs(None))));
        assert_eq!(read("hex://labs/"), Some(Ok(Target::Labs(None))));
        assert_eq!(
            read("hex://labs/live-text"),
            Some(Ok(Target::Labs(Some("live-text".into()))))
        );
        assert_eq!(
            read("hex://labs/Live-Text/"),
            Some(Ok(Target::Labs(Some("live-text".into()))))
        );
        assert_eq!(
            parse("hex-test://labs/meetings", "hex-test"),
            Some(Ok(Target::Labs(Some("meetings".into()))))
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
            assert_eq!(
                read(&link),
                Some(Ok(Target::Labs(Some(feature.name().into())))),
                "{link}"
            );
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
