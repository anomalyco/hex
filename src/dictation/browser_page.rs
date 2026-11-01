//! A browser's open page, read from its Accessibility tree when its
//! scripting dictionary can't be asked (Automation off). The walk itself is
//! here, over any tree, so it is tested on every OS; `accessibility` feeds it
//! the live one on macOS.
//!
//! Chromium browsers behind another app often have no focused element and
//! no focused window, and Arc's native window chrome (sidebar, toolbar)
//! sits between the window and the page. So every window is searched,
//! breadth first and bounded, for its web areas, and each window's and web
//! area's title is kept: a call's tab title (`Meet - abc-defg-hij`) names it
//! even when no address does (#827, #831).

use std::collections::VecDeque;

/// One element of an Accessibility tree, as the walk reads it.
pub(crate) trait AxNode: Sized {
    fn role(&self) -> Option<String>;
    fn title(&self) -> Option<String>;
    /// The page address of a web area (`AXURL`).
    fn url(&self) -> Option<String>;
    fn children(&self) -> Vec<Self>;
}

/// The pages a browser shows: addresses and titles, the most likely first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct BrowserPages {
    pub(crate) urls: Vec<String>,
    pub(crate) titles: Vec<String>,
}

impl BrowserPages {
    fn push_url(&mut self, url: Option<String>) {
        if let Some(url) = url.filter(|url| !url.is_empty())
            && !self.urls.contains(&url)
        {
            self.urls.push(url);
        }
    }

    fn push_title(&mut self, title: Option<String>) {
        if let Some(title) = title
            .map(|title| title.trim().to_string())
            .filter(|title| !title.is_empty())
            && !self.titles.contains(&title)
        {
            self.titles.push(title);
        }
    }
}

/// How many elements one window's search reads at most. Arc's chrome is a
/// few hundred; a page's own content is never entered.
const MAX_NODES_PER_WINDOW: usize = 400;
const MAX_DEPTH: usize = 40;

/// Gathers the pages of `windows` (focused or main first): each window's
/// title, then its web areas' addresses and titles. A web area's contents
/// are not searched.
pub(crate) fn pages<N: AxNode>(windows: impl IntoIterator<Item = N>) -> BrowserPages {
    let mut pages = BrowserPages::default();
    for window in windows {
        pages.push_title(window.title());
        let mut queue = VecDeque::from([(window, 0)]);
        let mut read = 0;
        while let Some((node, depth)) = queue.pop_front() {
            read += 1;
            if read > MAX_NODES_PER_WINDOW {
                break;
            }
            if node.role().as_deref() == Some("AXWebArea") {
                pages.push_url(node.url());
                pages.push_title(node.title());
                continue;
            }
            if depth < MAX_DEPTH {
                queue.extend(node.children().into_iter().map(|child| (child, depth + 1)));
            }
        }
    }
    pages
}

/// The address and title to judge a call by: the first that `names_call`
/// accepts, else the first of each.
pub(crate) fn choose(
    pages: &BrowserPages,
    url_names_call: impl Fn(&str) -> bool,
    title_names_call: impl Fn(&str) -> bool,
) -> (Option<String>, Option<String>) {
    let url = pages
        .urls
        .iter()
        .find(|url| url_names_call(url))
        .or_else(|| pages.urls.first())
        .cloned();
    let title = pages
        .titles
        .iter()
        .find(|title| title_names_call(title))
        .or_else(|| pages.titles.first())
        .cloned();
    (url, title)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A captured element: role, title, URL, children.
    #[derive(Clone)]
    struct Fixture(
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        Vec<Fixture>,
    );

    impl AxNode for Fixture {
        fn role(&self) -> Option<String> {
            Some(self.0.into())
        }
        fn title(&self) -> Option<String> {
            self.1.map(Into::into)
        }
        fn url(&self) -> Option<String> {
            self.2.map(Into::into)
        }
        fn children(&self) -> Vec<Self> {
            self.3.clone()
        }
    }

    fn element(role: &'static str, children: Vec<Fixture>) -> Fixture {
        Fixture(role, None, None, children)
    }

    /// Arc's shape: the page sits under the window's split view beside the
    /// sidebar, and the window title is the space, not the tab.
    fn arc_window() -> Fixture {
        Fixture(
            "AXWindow",
            Some("Work"),
            None,
            vec![element(
                "AXGroup",
                vec![element(
                    "AXSplitGroup",
                    vec![
                        element(
                            "AXGroup",
                            vec![
                                Fixture("AXTextField", Some("Search or Enter URL…"), None, vec![]),
                                Fixture("AXButton", Some("Meet - abc-defg-hij"), None, vec![]),
                            ],
                        ),
                        element(
                            "AXGroup",
                            vec![element(
                                "AXGroup",
                                vec![Fixture(
                                    "AXWebArea",
                                    Some("Meet - abc-defg-hij"),
                                    Some("https://meet.google.com/abc-defg-hij"),
                                    vec![element("AXGroup", vec![])],
                                )],
                            )],
                        ),
                    ],
                )],
            )],
        )
    }

    #[test]
    fn arcs_page_is_found_beside_its_sidebar() {
        let little_arc = Fixture(
            "AXWindow",
            Some("Docs"),
            None,
            vec![Fixture(
                "AXWebArea",
                Some("Docs"),
                Some("https://docs.example.com/"),
                vec![],
            )],
        );
        let pages = pages([little_arc, arc_window()]);
        assert_eq!(
            pages.urls,
            [
                "https://docs.example.com/",
                "https://meet.google.com/abc-defg-hij"
            ]
        );
        assert_eq!(pages.titles, ["Docs", "Work", "Meet - abc-defg-hij"]);
        let (url, title) = choose(
            &pages,
            |url| url.contains("meet.google.com/"),
            |title| title.starts_with("Meet - "),
        );
        assert_eq!(url.as_deref(), Some("https://meet.google.com/abc-defg-hij"));
        assert_eq!(title.as_deref(), Some("Meet - abc-defg-hij"));
    }

    #[test]
    fn a_window_without_its_web_tree_still_gives_its_title() {
        // Chromium behind another app, its web accessibility still off.
        let pages = pages([Fixture(
            "AXWindow",
            Some("Meet - abc-defg-hij"),
            None,
            vec![],
        )]);
        assert!(pages.urls.is_empty());
        assert_eq!(
            choose(&pages, |_| false, |_| false).1.as_deref(),
            Some("Meet - abc-defg-hij")
        );
    }

    #[test]
    fn nothing_found_is_nothing_chosen() {
        let pages = pages(Vec::<Fixture>::new());
        assert_eq!(choose(&pages, |_| true, |_| true), (None, None));
    }
}
