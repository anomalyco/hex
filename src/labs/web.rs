//! The labs Hex's website lists: `site/src/labs.json`, from which the site
//! builds a page and a social card per lab at `/hex/labs/<id>`. Only the
//! test below reads it: it fails when the file is out of date with this
//! registry, and `HEX_UPDATE_LABS_JSON=1 scripts/check labs::web` rewrites it.

use super::{Audience, Feature, Os};

impl Feature {
    /// Whether Hex's website lists it: every public build has it. Dev
    /// features are left out, and so are the ones only builds with the
    /// capture build option show.
    const fn on_the_web(self) -> bool {
        matches!(self.audience(), Audience::Everyone)
            && !matches!(
                self,
                Self::Screenshots | Self::AnnotateReplay | Self::CopyCaptureZip
            )
    }
}

impl Os {
    const fn id(self) -> &'static str {
        match self {
            Self::Macos => "macos",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }
}

/// `site/src/labs.json`: the website's labs, in the registry's order.
fn catalog_json() -> String {
    let labs: Vec<_> = Feature::ALL
        .into_iter()
        .filter(|feature| feature.on_the_web())
        .map(|feature| {
            let added = feature.added();
            serde_json::json!({
                "id": feature.name(),
                "title": feature.title(),
                "summary": feature.summary(),
                "group": feature.group().id(),
                "groupTitle": feature.group().title(),
                "oses": feature.oses().iter().map(|os| os.id()).collect::<Vec<_>>(),
                "added": format!("{}-{:02}-{:02}", added / 10000, added / 100 % 100, added % 100),
            })
        })
        .collect();
    let mut json = serde_json::to_string_pretty(&serde_json::json!({ "labs": labs }))
        .expect("the catalog serializes");
    json.push('\n');
    json
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/site/src/labs.json");

    #[test]
    fn the_website_lists_every_public_lab() {
        let expected = catalog_json();
        if std::env::var_os("HEX_UPDATE_LABS_JSON").is_some() {
            std::fs::write(PATH, &expected).unwrap();
            return;
        }
        let saved = std::fs::read_to_string(PATH).unwrap_or_default();
        assert!(
            saved == expected,
            "site/src/labs.json is out of date with the Labs registry; \
             run `HEX_UPDATE_LABS_JSON=1 scripts/check labs::web` and commit it"
        );
    }

    #[test]
    fn the_website_leaves_out_developer_and_capture_build_labs() {
        assert!(Feature::YoloMode.on_the_web());
        for feature in [
            Feature::Playground,
            Feature::VoiceFilter,
            Feature::Screenshots,
            Feature::AnnotateReplay,
            Feature::CopyCaptureZip,
        ] {
            assert!(!feature.on_the_web(), "{}", feature.name());
        }
        for feature in Feature::ALL {
            assert!(!feature.oses().is_empty(), "{} has no OS", feature.name());
            assert!(!feature.title().is_empty() && !feature.summary().is_empty());
        }
    }
}
