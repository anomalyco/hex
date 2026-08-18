//! Embedded asset source for gpui. gpui-component resolves icon names to
//! `icons/<name>.svg` paths but ships no SVG files; the Lucide subset the
//! adopted components reference is embedded here.

use std::borrow::Cow;

use gpui::{AssetSource, Result, SharedString};

pub struct HexAssets;

const ICONS: &[(&str, &[u8])] = &[
    (
        "icons/check.svg",
        include_bytes!("../../assets/icons/lucide/check.svg"),
    ),
    (
        "icons/chevron-down.svg",
        include_bytes!("../../assets/icons/lucide/chevron-down.svg"),
    ),
    (
        "icons/circle-x.svg",
        include_bytes!("../../assets/icons/lucide/circle-x.svg"),
    ),
    (
        "icons/inbox.svg",
        include_bytes!("../../assets/icons/lucide/inbox.svg"),
    ),
    (
        "icons/loader-circle.svg",
        include_bytes!("../../assets/icons/lucide/loader-circle.svg"),
    ),
    (
        "icons/search.svg",
        include_bytes!("../../assets/icons/lucide/search.svg"),
    ),
];

impl AssetSource for HexAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        Ok(ICONS
            .iter()
            .find(|(candidate, _)| *candidate == path)
            .map(|(_, bytes)| Cow::Borrowed(*bytes)))
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        Ok(ICONS
            .iter()
            .filter(|(candidate, _)| candidate.starts_with(path))
            .map(|(candidate, _)| (*candidate).into())
            .collect())
    }
}
