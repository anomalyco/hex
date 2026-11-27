//! A plain snapshot of an app's Accessibility tree, and the bounded walk
//! that fills it. Nothing here names an AppKit or Core Foundation type, so a
//! snapshot is a fixture as readily as a live read: [`super::live`] feeds the
//! walk the real tree, the tests feed it hand-written ones.
//!
//! Web content adds two attributes to the usual role, title, description and
//! value: the element's DOM id and class list (`AXDOMIdentifier`,
//! `AXDOMClassList`), which is where a call app's page says whose tile an
//! element is and whether it is marked as speaking.

use std::collections::{HashSet, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// One element. Empty attributes are left out of its JSON.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Node {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) subrole: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) dom_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) dom_classes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) children: Vec<Node>,
}

impl Node {
    /// Whether the element holds a password: its value is never asked for
    /// and nothing below it is read.
    fn is_secure(&self) -> bool {
        [&self.role, &self.subrole]
            .into_iter()
            .any(|name| name.as_deref() == Some("AXSecureTextField"))
    }

    /// Whether a role belongs to the application shell rather than a window's
    /// content (`AXApplication`, `AXMenuBar`): never descended into during a
    /// window walk.
    pub(super) fn is_shell_role(role: Option<&str>) -> bool {
        matches!(role, Some("AXApplication" | "AXMenuBar"))
    }

    /// Whether the walk stops at this element without reading its children.
    fn stops_descent(&self) -> bool {
        Self::is_shell_role(self.role.as_deref())
    }
}

/// What one element answers: its own attributes (no value, no children yet),
/// at most the asked number of child elements, and how many more it has.
pub(crate) struct Read<E> {
    pub(crate) node: Node,
    pub(crate) children: Vec<E>,
    pub(crate) dropped: usize,
}

/// One element of a tree as the walk reads it.
pub(crate) trait Element: Sized {
    /// Identity within a single walk. Any element whose identity has already
    /// been seen is skipped so cycles in the Accessibility graph cannot loop.
    type Id: Eq + Hash;

    /// The element's identity.
    fn id(&self) -> Self::Id;
    /// The element's attributes and up to `room` children. `None` when the
    /// app did not answer for it.
    fn read(&self, room: usize) -> Option<Read<Self>>;
    /// The element's value. Asked only after [`Self::read`] showed it is no
    /// password field.
    fn value(&self) -> Option<String>;
}

/// How much one snapshot may read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Budgets {
    /// Elements asked, across every window.
    pub(crate) max_nodes: usize,
    /// Levels below a window.
    pub(crate) max_depth: usize,
    /// Wall-clock time, checked between elements.
    pub(crate) deadline: Duration,
}

/// A budget that stopped a snapshot before the whole tree was read.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Cut {
    Nodes,
    Depth,
    Deadline,
}

/// Every window of one app, read breadth first so a cut keeps the shallow
/// levels of every window rather than all of the first.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct Snapshot {
    pub(crate) windows: Vec<Node>,
    /// Elements asked, answered or not.
    pub(crate) nodes: usize,
    /// Elements the app did not answer for; they are not in the tree.
    pub(crate) unreadable: usize,
    /// The budgets that cut the read short; empty for a whole tree.
    pub(crate) cut_short: Vec<Cut>,
}

impl Snapshot {
    fn cut(&mut self, by: Cut) {
        if !self.cut_short.contains(&by) {
            self.cut_short.push(by);
        }
    }
}

pub(crate) fn snapshot<E: Element>(
    windows: impl IntoIterator<Item = E>,
    budgets: Budgets,
) -> Snapshot {
    let started = Instant::now();
    let mut visited: HashSet<E::Id> = HashSet::new();
    let mut queue: VecDeque<(E, Option<usize>, usize)> = VecDeque::new();
    for window in windows {
        if visited.insert(window.id()) {
            queue.push_back((window, None, 0));
        }
    }
    // Breadth-first order: a parent always comes before its children.
    let mut read: Vec<(Node, Option<usize>)> = Vec::new();
    let mut snapshot = Snapshot::default();
    while let Some((element, parent, depth)) = queue.pop_front() {
        if snapshot.nodes >= budgets.max_nodes {
            snapshot.cut(Cut::Nodes);
            break;
        }
        if started.elapsed() >= budgets.deadline {
            snapshot.cut(Cut::Deadline);
            break;
        }
        snapshot.nodes += 1;
        let deeper = depth < budgets.max_depth;
        let room = if deeper {
            budgets
                .max_nodes
                .saturating_sub(snapshot.nodes + queue.len())
        } else {
            0
        };
        let Some(answer) = element.read(room) else {
            snapshot.unreadable += 1;
            continue;
        };
        let mut node = answer.node;
        node.children.clear();
        if node.is_secure() {
            node.value = None;
            read.push((node, parent));
            continue;
        }
        node.value = element.value();
        if node.stops_descent() {
            read.push((node, parent));
            continue;
        }
        if answer.dropped > 0 {
            snapshot.cut(if deeper { Cut::Nodes } else { Cut::Depth });
        }
        let index = read.len();
        read.push((node, parent));
        for child in answer.children {
            if visited.insert(child.id()) {
                queue.push_back((child, Some(index), depth + 1));
            }
        }
    }
    snapshot.windows = assemble(read);
    snapshot
}

/// Nests breadth-first `(node, parent)` pairs. Taken from the end, every
/// node already holds its own children (in reverse) when it moves into its
/// parent.
fn assemble(mut read: Vec<(Node, Option<usize>)>) -> Vec<Node> {
    let mut windows = Vec::new();
    while let Some((mut node, parent)) = read.pop() {
        node.children.reverse();
        match parent {
            Some(parent) => read[parent].0.children.push(node),
            None => windows.push(node),
        }
    }
    windows.reverse();
    windows
}

/// How much web content a snapshot holds. A Chromium or Electron app that no
/// assistive client has asked shows its windows and native chrome but no web
/// area, or an empty one.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) struct WebContent {
    /// `AXWebArea` elements.
    pub(crate) areas: usize,
    /// Elements below a web area.
    pub(crate) nodes: usize,
}

pub(crate) fn web_content(windows: &[Node]) -> WebContent {
    let mut content = WebContent::default();
    let mut stack: Vec<(&Node, bool)> = windows.iter().map(|window| (window, false)).collect();
    while let Some((node, inside)) = stack.pop() {
        if inside {
            content.nodes += 1;
        }
        let area = node.role.as_deref() == Some("AXWebArea");
        if area {
            content.areas += 1;
        }
        stack.extend(node.children.iter().map(|child| (child, inside || area)));
    }
    content
}

/// Which elements a watch reports. An element matches when any criterion
/// given matches it; a filter with none matches nothing.
#[derive(Clone, Debug, Default)]
pub(crate) struct Filter {
    /// The DOM id starts with this.
    pub(crate) id_prefix: Option<String>,
    /// One DOM class contains this.
    pub(crate) class_contains: Option<String>,
    /// The title, description or value matches this.
    pub(crate) label: Option<regex::Regex>,
}

impl Filter {
    fn matches(&self, node: &Node) -> bool {
        let id = self.id_prefix.as_deref().is_some_and(|prefix| {
            node.dom_id
                .as_deref()
                .is_some_and(|id| id.starts_with(prefix))
        });
        let class = self
            .class_contains
            .as_deref()
            .is_some_and(|part| node.dom_classes.iter().any(|class| class.contains(part)));
        let label = self.label.as_ref().is_some_and(|pattern| {
            [&node.title, &node.description, &node.value]
                .into_iter()
                .flatten()
                .any(|text| pattern.is_match(text))
        });
        id || class || label
    }
}

/// One matching element, without its children.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct Match {
    /// Where it sits in this read: the window's index, then each child's
    /// index on the way down (`0/3/1`). An element is below another when
    /// its path starts with the other's.
    pub(crate) path: String,
    #[serde(flatten)]
    pub(crate) node: Node,
}

/// The elements of `windows` that `filter` matches, in document order.
pub(crate) fn matches(windows: &[Node], filter: &Filter) -> Vec<Match> {
    let mut found = Vec::new();
    let mut stack: Vec<(&Node, String)> = windows
        .iter()
        .enumerate()
        .rev()
        .map(|(index, window)| (window, index.to_string()))
        .collect();
    while let Some((node, path)) = stack.pop() {
        stack.extend(
            node.children
                .iter()
                .enumerate()
                .rev()
                .map(|(index, child)| (child, format!("{path}/{index}"))),
        );
        if filter.matches(node) {
            found.push(Match {
                path,
                node: Node {
                    children: Vec::new(),
                    ..node.clone()
                },
            });
        }
    }
    found
}

#[cfg(test)]
mod tests;
