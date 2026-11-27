use std::cell::Cell;

use super::*;

/// A fixture element: a node of a hand-written tree. A node whose role is
/// `Unreadable` does not answer, as an app that timed out would not.
struct Fake<'a> {
    node: &'a Node,
    values_asked: &'a Cell<usize>,
}

impl Element for Fake<'_> {
    type Id = usize;

    fn id(&self) -> usize {
        std::ptr::from_ref(self.node) as usize
    }

    fn read(&self, room: usize) -> Option<Read<Self>> {
        if self.node.role.as_deref() == Some("Unreadable") {
            return None;
        }
        Some(Read {
            node: Node {
                value: None,
                children: Vec::new(),
                ..self.node.clone()
            },
            children: self
                .node
                .children
                .iter()
                .take(room)
                .map(|node| Fake {
                    node,
                    values_asked: self.values_asked,
                })
                .collect(),
            dropped: self.node.children.len().saturating_sub(room),
        })
    }

    fn value(&self) -> Option<String> {
        self.values_asked.set(self.values_asked.get() + 1);
        self.node.value.clone()
    }
}

const WHOLE: Budgets = Budgets {
    max_nodes: 10_000,
    max_depth: 100,
    deadline: Duration::from_secs(60),
};

fn read(windows: &[Node], budgets: Budgets) -> Snapshot {
    let values_asked = Cell::new(0);
    snapshot(
        windows.iter().map(|node| Fake {
            node,
            values_asked: &values_asked,
        }),
        budgets,
    )
}

fn count(windows: &[Node]) -> usize {
    windows.iter().map(|node| 1 + count(&node.children)).sum()
}

/// A huddle window as other tools describe Slack's: one grid cell per
/// person, whose DOM id ends in a user id, holding a profile button, an
/// audio label, and an overlay that carries a speaking class. The shape is a
/// claim the probe exists to check, and the people are invented.
fn huddle() -> Vec<Node> {
    serde_json::from_str(include_str!("fixtures/huddle.json")).unwrap()
}

#[test]
fn a_whole_tree_reads_back_as_it_is() {
    let windows = huddle();
    let snapshot = read(&windows, WHOLE);
    assert_eq!(snapshot.windows, windows);
    assert_eq!(snapshot.nodes, count(&windows));
    assert_eq!(snapshot.unreadable, 0);
    assert!(snapshot.cut_short.is_empty());
}

#[test]
fn a_fixture_names_each_tile_its_person_and_who_speaks() {
    let windows = read(&huddle(), WHOLE).windows;
    let tiles = matches(
        &windows,
        &Filter {
            id_prefix: Some("huddle-grid-gridcell-".into()),
            ..Default::default()
        },
    );
    assert_eq!(
        tiles
            .iter()
            .map(|tile| tile.node.dom_id.as_deref().unwrap())
            .collect::<Vec<_>>(),
        [
            "huddle-grid-gridcell-self_U0AAAAAAA01",
            "huddle-grid-gridcell-5b1f0c52-7d1e-4a55-9a37-0d6f6f3c2e10_U0BBBBBBB02",
        ]
    );
    assert!(tiles.iter().all(|tile| tile.node.children.is_empty()));

    let speaking = matches(
        &windows,
        &Filter {
            class_contains: Some("active_speaker".into()),
            ..Default::default()
        },
    );
    assert_eq!(speaking.len(), 1);
    // The speaking mark sits below the second tile, not the first.
    assert!(speaking[0].path.starts_with(&format!("{}/", tiles[1].path)));
    assert!(!speaking[0].path.starts_with(&format!("{}/", tiles[0].path)));

    let labels = matches(
        &windows,
        &Filter {
            label: Some(regex::Regex::new("'s profile$|^audio is (on|off)$").unwrap()),
            ..Default::default()
        },
    );
    assert_eq!(
        labels
            .iter()
            .map(|label| label
                .node
                .description
                .as_deref()
                .or(label.node.title.as_deref())
                .unwrap())
            .collect::<Vec<_>>(),
        [
            "View Mira Okonkwo's profile",
            "audio is off",
            "View Teodor Blanc's profile",
            "audio is on",
        ]
    );
}

#[test]
fn any_criterion_given_matches_and_none_given_matches_nothing() {
    let windows = huddle();
    assert!(matches(&windows, &Filter::default()).is_empty());
    let either = matches(
        &windows,
        &Filter {
            id_prefix: Some("huddle-grid-gridcell-self_".into()),
            class_contains: Some("active_speaker".into()),
            label: None,
        },
    );
    assert_eq!(either.len(), 2);
    // Document order: the local tile comes before the other tile's overlay.
    assert!(either[0].node.dom_id.is_some());
    assert!(either[1].node.dom_id.is_none());
}

#[test]
fn the_node_budget_keeps_the_shallow_levels_and_says_it_cut() {
    let windows = huddle();
    let snapshot = read(
        &windows,
        Budgets {
            max_nodes: 4,
            ..WHOLE
        },
    );
    assert_eq!(snapshot.nodes, 4);
    assert_eq!(count(&snapshot.windows), 4);
    assert_eq!(snapshot.cut_short, [Cut::Nodes]);
    // Both windows are there before either window's grandchildren.
    assert_eq!(snapshot.windows.len(), 2);
}

#[test]
fn the_depth_budget_stops_below_its_level_and_says_it_cut() {
    let windows = huddle();
    let snapshot = read(
        &windows,
        Budgets {
            max_depth: 1,
            ..WHOLE
        },
    );
    assert_eq!(snapshot.cut_short, [Cut::Depth]);
    assert!(snapshot.windows.iter().all(|window| {
        window
            .children
            .iter()
            .all(|child| child.children.is_empty())
    }));
    assert!(
        snapshot
            .windows
            .iter()
            .any(|window| !window.children.is_empty())
    );
}

#[test]
fn a_tree_that_fits_its_depth_exactly_is_whole() {
    let windows = [Node {
        role: Some("AXWindow".into()),
        children: vec![Node::default()],
        ..Default::default()
    }];
    let snapshot = read(
        &windows,
        Budgets {
            max_depth: 1,
            ..WHOLE
        },
    );
    assert!(snapshot.cut_short.is_empty());
    assert_eq!(snapshot.nodes, 2);
}

#[test]
fn a_spent_deadline_reads_nothing_and_says_it_cut() {
    let snapshot = read(
        &huddle(),
        Budgets {
            deadline: Duration::ZERO,
            ..WHOLE
        },
    );
    assert_eq!(snapshot.nodes, 0);
    assert!(snapshot.windows.is_empty());
    assert_eq!(snapshot.cut_short, [Cut::Deadline]);
}

#[test]
fn a_password_field_gives_no_value_and_nothing_below_it() {
    let field = |role: Option<&str>, subrole: Option<&str>| Node {
        role: role.map(Into::into),
        subrole: subrole.map(Into::into),
        value: Some("hunter2".into()),
        children: vec![Node {
            role: Some("AXStaticText".into()),
            value: Some("hunter2".into()),
            ..Default::default()
        }],
        ..Default::default()
    };
    let windows = [Node {
        role: Some("AXWindow".into()),
        children: vec![
            field(Some("AXTextField"), Some("AXSecureTextField")),
            field(Some("AXSecureTextField"), None),
        ],
        ..Default::default()
    }];
    let values_asked = Cell::new(0);
    let snapshot = snapshot(
        windows.iter().map(|node| Fake {
            node,
            values_asked: &values_asked,
        }),
        WHOLE,
    );
    // Only the window's own value was asked for.
    assert_eq!(values_asked.get(), 1);
    assert_eq!(snapshot.nodes, 3);
    assert!(snapshot.cut_short.is_empty());
    for field in &snapshot.windows[0].children {
        assert_eq!(field.value, None);
        assert!(field.children.is_empty());
    }
    assert!(
        !serde_json::to_string(&snapshot.windows)
            .unwrap()
            .contains("hunter2")
    );
}

#[test]
fn an_element_that_does_not_answer_is_counted_and_left_out() {
    let windows = [Node {
        role: Some("AXWindow".into()),
        children: vec![
            Node {
                role: Some("Unreadable".into()),
                children: vec![Node::default()],
                ..Default::default()
            },
            Node {
                role: Some("AXButton".into()),
                ..Default::default()
            },
        ],
        ..Default::default()
    }];
    let snapshot = read(&windows, WHOLE);
    assert_eq!(snapshot.nodes, 3);
    assert_eq!(snapshot.unreadable, 1);
    assert_eq!(snapshot.windows[0].children.len(), 1);
    assert_eq!(
        snapshot.windows[0].children[0].role.as_deref(),
        Some("AXButton")
    );
}

#[test]
fn web_content_counts_what_sits_below_a_web_area() {
    let windows = huddle();
    let content = web_content(&windows);
    assert_eq!(content.areas, 1);
    assert_eq!(content.nodes, 10);

    // A Chromium window nobody asked for its web tree: chrome, no page.
    let chrome_only: Vec<Node> = serde_json::from_str(
        r#"[{"role":"AXWindow","title":"Huddle","children":[
            {"role":"AXGroup","children":[{"role":"AXWebArea"}]},
            {"role":"AXButton","subrole":"AXCloseButton"}]}]"#,
    )
    .unwrap();
    assert_eq!(web_content(&chrome_only), WebContent { areas: 1, nodes: 0 });
    assert_eq!(web_content(&[]), WebContent::default());
}

#[test]
fn a_node_serializes_only_what_it_has() {
    let node = Node {
        role: Some("AXGroup".into()),
        dom_id: Some("tile".into()),
        dom_classes: vec!["a".into(), "b".into()],
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_string(&node).unwrap(),
        r#"{"role":"AXGroup","dom_id":"tile","dom_classes":["a","b"]}"#
    );
    let found = Match {
        path: "0/1".into(),
        node,
    };
    assert_eq!(
        serde_json::to_string(&found).unwrap(),
        r#"{"path":"0/1","role":"AXGroup","dom_id":"tile","dom_classes":["a","b"]}"#
    );
    assert_eq!(
        serde_json::from_str::<Match>(&serde_json::to_string(&found).unwrap()).unwrap(),
        found
    );
}

struct GraphNode {
    role: &'static str,
    title: Option<&'static str>,
    children: Vec<usize>,
}

struct GraphRef<'a> {
    graph: &'a [GraphNode],
    index: usize,
}

impl Element for GraphRef<'_> {
    type Id = usize;

    fn id(&self) -> usize {
        self.index
    }

    fn read(&self, room: usize) -> Option<Read<Self>> {
        let entry = &self.graph[self.index];
        Some(Read {
            node: Node {
                role: Some(entry.role.into()),
                title: entry.title.map(Into::into),
                ..Default::default()
            },
            children: entry
                .children
                .iter()
                .take(room)
                .map(|&index| GraphRef {
                    graph: self.graph,
                    index,
                })
                .collect(),
            dropped: entry.children.len().saturating_sub(room),
        })
    }

    fn value(&self) -> Option<String> {
        None
    }
}

#[test]
fn a_cyclic_graph_terminates_cleanly_without_recursing_infinitely() {
    // 0 (AXWindow) -> [0 (self-loop), 1 (AXGroup), 2 (AXGroup)]
    // 1 (AXGroup)  -> [0 (back-edge), 3 (shared leaf)]
    // 2 (AXGroup)  -> [1 (cross-edge), 3 (shared leaf)]
    // 3 (AXButton) -> [3 (self-loop)]
    let graph = [
        GraphNode {
            role: "AXWindow",
            title: Some("Call"),
            children: vec![0, 1, 2],
        },
        GraphNode {
            role: "AXGroup",
            title: Some("A"),
            children: vec![0, 3],
        },
        GraphNode {
            role: "AXGroup",
            title: Some("B"),
            children: vec![1, 3],
        },
        GraphNode {
            role: "AXButton",
            title: Some("Mute"),
            children: vec![3],
        },
    ];
    let result = snapshot(
        [
            GraphRef {
                graph: &graph,
                index: 0,
            },
            // Duplicate root in the window list is also skipped.
            GraphRef {
                graph: &graph,
                index: 0,
            },
        ],
        WHOLE,
    );
    assert!(result.cut_short.is_empty());
    assert_eq!(result.nodes, 4);
    assert_eq!(count(&result.windows), 4);
    assert_eq!(result.windows.len(), 1);
    assert_eq!(result.windows[0].children.len(), 2);
    assert_eq!(result.windows[0].children[0].children.len(), 1);
    assert_eq!(
        result.windows[0].children[0].children[0].title.as_deref(),
        Some("Mute")
    );
    assert!(result.windows[0].children[1].children.is_empty());
}

#[test]
fn application_and_menu_bar_children_are_not_recursed_into() {
    let windows = [Node {
        role: Some("AXWindow".into()),
        title: Some("Main".into()),
        children: vec![
            Node {
                role: Some("AXApplication".into()),
                children: vec![Node {
                    role: Some("AXMenuBar".into()),
                    children: vec![Node {
                        role: Some("AXMenuBarItem".into()),
                        title: Some("File".into()),
                        ..Default::default()
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            },
            Node {
                role: Some("AXMenuBar".into()),
                children: vec![
                    Node {
                        role: Some("AXMenuBarItem".into()),
                        title: Some("Edit".into()),
                        ..Default::default()
                    },
                    Node {
                        role: Some("AXMenuBarItem".into()),
                        title: Some("View".into()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
            Node {
                role: Some("AXButton".into()),
                title: Some("Leave".into()),
                ..Default::default()
            },
        ],
        ..Default::default()
    }];
    for budgets in [
        WHOLE,
        Budgets {
            max_depth: 1,
            ..WHOLE
        },
    ] {
        let result = read(&windows, budgets);
        // Neither AXApplication nor AXMenuBar descends, and their ignored
        // children do not falsely report a depth cut.
        assert!(result.cut_short.is_empty());
        assert_eq!(result.nodes, 4);
        assert_eq!(result.windows[0].children.len(), 3);
        assert!(result.windows[0].children[0].children.is_empty());
        assert!(result.windows[0].children[1].children.is_empty());
        assert_eq!(
            result.windows[0].children[2].title.as_deref(),
            Some("Leave")
        );
    }
}

#[test]
fn deep_reply_json_up_to_default_max_depth_deserializes_cleanly() {
    use crate::accessibility::probe::{Reading, Reply};

    let mut deep = Node {
        role: Some("AXStaticText".into()),
        title: Some("bottom".into()),
        ..Default::default()
    };
    for level in (0..80).rev() {
        deep = Node {
            role: Some(if level == 0 { "AXWindow" } else { "AXGroup" }.into()),
            children: vec![deep],
            ..Default::default()
        };
    }
    let reply = Reply::Read(Reading {
        pid: 7,
        frontmost: Some(true),
        elapsed_ms: 2.5,
        windows: 1,
        nodes: 81,
        unreadable: 0,
        cut_short: Vec::new(),
        web_areas: 0,
        web_nodes: 0,
        web_accessibility_requested: None,
        matches: None,
        tree: Some(vec![deep]),
    });
    let json = serde_json::to_vec(&reply).unwrap();
    assert!(serde_json::from_slice::<Reply>(&json).is_err());
    assert_eq!(Reply::from_json_slice(&json).unwrap(), reply);
}
