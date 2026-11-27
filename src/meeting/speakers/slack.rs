//! Slack huddles, as Slack's desktop app shows them.
//!
//! Slack is an Electron app, so its huddle is a web page, and what its
//! Accessibility tree says of the page is the page's own markup: each
//! person on the huddle is a grid cell with the class [`TILE`], in the
//! full gallery and in the compact view beside a channel alike. A tile's
//! DOM id ends in the person's Slack user id, and the user's own tile says
//! `self` where everyone else's has a connection id. While someone speaks,
//! an overlay inside their tile carries the class [`SPEAKING`], and loses
//! it as soon as they stop.
//!
//! The page is there to read only once Slack has been asked for its web
//! accessibility tree, which is the reader's business. Everything here is
//! one pure function of the snapshot.
//!
//! The Slack user id tells one person's tiles from another's and goes no
//! further than an [`AppParticipantId`]: it is never written or logged.

use super::SpeakerName;
use super::adapter::{AppParticipantId, Observation, ObservedParticipant};
use super::evidence::Untrusted;
use crate::accessibility::tree::{Node, Snapshot};

/// Slack's desktop app, by its canonical bundle id.
pub const BUNDLE_ID: &str = "com.tinyspeck.slackmacgap";

/// Reads a huddle from Slack's windows.
pub const ADAPTER: fn(&Snapshot) -> Observation = observe;

/// The class of one person's tile.
const TILE: &str = "p-huddle_peer_tile";
/// The class of the part of a tile that shows the person's name.
const NAME_OVERLAY: &str = "p-huddle_peer_tile__name_overlay";
/// The class an overlay inside a tile carries while its person speaks.
const SPEAKING: &str = "p-huddle_peer_tile__overlay--active_speaker";
/// What every tile's DOM id starts with.
const TILE_ID: &str = "huddle-grid-gridcell-";
/// What the user's own tile's DOM id starts with.
const OWN_TILE_ID: &str = "huddle-grid-gridcell-self_";

/// One call is one window with tiles in it. Two such windows are two views
/// that may not agree, and neither is believed.
fn observe(snapshot: &Snapshot) -> Observation {
    let mut calls = snapshot
        .windows
        .iter()
        .map(tiles)
        .filter(|tiles| !tiles.is_empty());
    let Some(tiles) = calls.next() else {
        return Observation::NoCallWindow;
    };
    if calls.next().is_some() {
        return Observation::Untrusted(Untrusted::TwoWindows);
    }
    huddle(&tiles)
}

/// Who the tiles of one window show, and who they mark as speaking.
///
/// Someone holds the floor only when they are the one person marked. With
/// two people marked at once, or a mark on a tile that names nobody, the
/// floor is nobody's, and what is said then stays unnamed.
fn huddle(tiles: &[&Node]) -> Observation {
    let mut roster: Vec<ObservedParticipant> = Vec::new();
    let mut speaking: Vec<AppParticipantId> = Vec::new();
    let mut unknown_speaker = false;
    for tile in tiles {
        let marked = below(tile).any(|node| has_class(node, SPEAKING));
        let Some((id, is_local)) = tile.dom_id.as_deref().and_then(person) else {
            unknown_speaker |= marked;
            continue;
        };
        if marked && !speaking.contains(&id) {
            speaking.push(id.clone());
        }
        // A second tile of someone already read adds only what the first
        // did not say.
        match roster.iter_mut().find(|known| known.id == id) {
            Some(known) => {
                known.is_local |= is_local;
                if known.name.is_none() {
                    known.name = name(tile);
                }
            }
            None => roster.push(ObservedParticipant {
                id,
                name: name(tile),
                is_local,
            }),
        }
    }
    let floor = match (speaking.as_slice(), unknown_speaker) {
        ([only], false) => Some(only.clone()),
        _ => None,
    };
    Observation::Active { roster, floor }
}

/// Whose tile a DOM id says it is, and whether it is the user's own.
///
/// The id ends in the person's Slack user id, after the connection it
/// names them on (`huddle-grid-gridcell-<connection>_<user>`). The user id
/// is the person: someone who drops and reconnects comes back on another
/// connection and is still themselves. An id of any other shape is taken
/// whole.
fn person(dom_id: &str) -> Option<(AppParticipantId, bool)> {
    let user = dom_id
        .strip_prefix(TILE_ID)
        .and_then(|tile| tile.rsplit_once('_'))
        .map(|(_, user)| user)
        .filter(|user| !user.trim().is_empty());
    let id = AppParticipantId::parse(user.unwrap_or(dom_id))?;
    Some((id, dom_id.starts_with(OWN_TILE_ID)))
}

/// What a tile calls its person: the text in its name overlay, or else the
/// name in the tile's own label ("View Alice's profile"), which is all the
/// user's own tile has.
fn name(tile: &Node) -> Option<SpeakerName> {
    let overlays = below(tile).filter(|node| has_class(node, NAME_OVERLAY));
    let shown = overlays.flat_map(below).find_map(|node| {
        let text = (node.role.as_deref() == Some("AXStaticText")).then_some(node.value.as_deref());
        text.flatten().and_then(SpeakerName::parse)
    });
    shown.or_else(|| {
        let labels = [tile.title.as_deref(), tile.description.as_deref()];
        let named = labels.into_iter().flatten().filter_map(profile_name);
        named.filter_map(SpeakerName::parse).next()
    })
}

/// The name in a tile's label, when the label is the one Slack gives a
/// tile. Any other label is not a name.
fn profile_name(label: &str) -> Option<&str> {
    let name = label.trim().strip_prefix("View ")?;
    ["'s profile", "\u{2019}s profile"]
        .into_iter()
        .find_map(|ending| name.strip_suffix(ending))
}

/// The tiles in one window, in the order the page has them. A tile is not
/// looked into for more tiles.
fn tiles(window: &Node) -> Vec<&Node> {
    let mut found = Vec::new();
    let mut stack = vec![window];
    while let Some(node) = stack.pop() {
        if node.role.as_deref() == Some("AXCell") && has_class(node, TILE) {
            found.push(node);
        } else {
            stack.extend(node.children.iter().rev());
        }
    }
    found
}

/// `node` and everything below it, in the order the page has them.
fn below(node: &Node) -> impl Iterator<Item = &Node> {
    let mut stack = vec![node];
    std::iter::from_fn(move || {
        let node = stack.pop()?;
        stack.extend(node.children.iter().rev());
        Some(node)
    })
}

fn has_class(node: &Node, class: &str) -> bool {
    node.dom_classes.iter().any(|own| own == class)
}

#[cfg(test)]
mod tests;
