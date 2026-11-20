//! The page a Windows browser window's title names, before meeting detection
//! reads it: a window title carries the browser's own name and Edge's profile
//! and other-tab count, while macOS and Linux report a tab's title.

use super::detection::{BROWSER_TITLE_SUFFIXES, clean_title};

/// Characters browsers put in their window titles that no one sees: Edge's
/// own name arrives as `Microsoft\u{200b} Edge`.
const ZERO_WIDTH: [char; 7] = [
    '\u{200b}', '\u{200c}', '\u{200d}', '\u{200e}', '\u{200f}', '\u{2060}', '\u{feff}',
];

/// The page a browser window's title names. A Windows window title is not a
/// tab's: the browser appends its name (Edge after its profile name and a
/// zero-width space), and Edge counts the window's other tabs ("and 2 more
/// pages", English only). Both are removed here, before detection reads the
/// title; macOS and Linux titles are tab titles and need no cleaning.
///
/// Without a profile segment, an Edge title cannot be told apart from a page
/// title that contains " - ", so a profile is dropped only when a page title
/// is still left.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub(crate) fn normalize_window_title(title: &str) -> String {
    let title: String = title
        .chars()
        .filter(|character| !ZERO_WIDTH.contains(character))
        .collect();
    let title = title.trim();
    let title = if let Some(page) = title.strip_suffix(" - Microsoft Edge") {
        strip_edge_profile(page)
    } else if let Some(page) = strip_chrome_profile(title) {
        page
    } else {
        clean_title(title, BROWSER_TITLE_SUFFIXES)
    };
    trim_more_pages(title).to_string()
}

/// Edge puts the profile's name between the page and the browser's own name,
/// and leaves it out for a single-profile window. It is dropped only when a
/// page title is still left, so `Meet - Design sync` keeps its name.
fn strip_edge_profile(title: &str) -> &str {
    title
        .rsplit_once(" - ")
        .filter(|(page, _)| page.contains(" - "))
        .map_or(title, |(page, _)| page)
}

/// Some Chrome windows put the profile's name after the browser's own
/// ("… - Google Chrome - Personal"). The browser's name marks the boundary,
/// so this needs no " - " guard like Edge's.
fn strip_chrome_profile(title: &str) -> Option<&str> {
    let (page, profile) = title.rsplit_once(" - Google Chrome")?;
    let profile = profile.strip_prefix(" - ")?;
    (!profile.is_empty() && !profile.contains(" - ")).then_some(page)
}

/// Leaves out a browser window's count of its other tabs ("and 2 more
/// pages"), which Edge puts right after the page title.
fn trim_more_pages(title: &str) -> &str {
    let Some((page, count)) = title.rsplit_once(" and ") else {
        return title;
    };
    let count = count
        .strip_suffix(" more pages")
        .or_else(|| count.strip_suffix(" more page"));
    match count {
        Some(count) if !count.is_empty() && count.bytes().all(|byte| byte.is_ascii_digit()) => page,
        _ => title,
    }
}
