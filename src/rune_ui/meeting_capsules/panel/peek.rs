//! The open recording's foot, under its copies: a link that opens the live
//! meeting in Hex's Meetings pane. It is the plain "Open in Hex" link for
//! the whole recording. The flag below keeps the variant that showed the
//! transcript's last lines under the call, anchored to the newest and
//! fading at the top, for whenever it earns its room.

use super::*;

/// Whether the foot shows the transcript's last lines once there are any.
/// `false` keeps the plain "Open in Hex" link for the whole recording.
const SHOWS_TRANSCRIPT: bool = false;

/// Lines of the transcript's end the foot shows.
const LINES: usize = 3;
/// `Paint::description`'s line.
const LINE: f32 = 16.0;
/// Above and below the lines, inside the link's hover fill.
const PEEK_PY: f32 = 8.0;
/// The oldest line shown fades into the panel.
const FADE: f32 = LINE;
/// The lines and the padding round them.
const PEEK_HEIGHT: f32 = LINES as f32 * LINE + 2.0 * PEEK_PY;
/// The plain link: as tall as the header at the panel's other end.
const LINK_HEIGHT: f32 = HEADER_HEIGHT;
/// The plain link and the gap over it.
pub(in crate::rune_ui::meeting_capsules) const LINK_FOOT: f32 = ROW_GAP + LINK_HEIGHT;
/// The transcript's last lines and the gap over them.
pub(in crate::rune_ui::meeting_capsules) const PEEK_FOOT: f32 = ROW_GAP + PEEK_HEIGHT;
/// The foot once the call has words.
pub(in crate::rune_ui::meeting_capsules) const WORDS_FOOT: f32 = if SHOWS_TRANSCRIPT {
    PEEK_FOOT
} else {
    LINK_FOOT
};
/// How much of the transcript's end is laid out: more than fills the lines,
/// so the oldest shown always runs under the fade.
const TAIL_CHARS: usize = 240;
/// The foot arrives after Stop and the two copies.
const ROW: usize = stage::ACTIONS - 1;

// The window every OS but Windows keeps holds the tallest open recording.
const _: () = assert!(
    OPEN_HEADER + NOTICE_HEIGHT + ROW_GAP + COPIES_HEIGHT + PEEK_FOOT + PANEL_PAD
        <= RECORDING_WINDOW.1 - 2.0 * CAPSULE_INSET
);

/// Whether the foot shows the call's words now.
fn shows_transcript(capsule: &Capsule) -> bool {
    SHOWS_TRANSCRIPT && !capsule.live.entries.is_empty()
}

/// The room the foot takes under the copies, its gap included.
pub(super) fn height(capsule: &Capsule) -> f32 {
    if capsule.live.entries.is_empty() {
        LINK_FOOT
    } else {
        WORDS_FOOT
    }
}

/// The foot: the transcript's last lines, or "Open in Hex", as one link.
pub(super) fn foot(p: Paint, capsule: &Capsule, shown: &Recording) -> El {
    let presence = capsule.action_presence(ROW).opacity;
    let cue = icon(&icons::OUTLINE_ARROW_UP_RIGHT, 14.0)
        .key("open-cue")
        .color(p.muted())
        .group_opacity(0.6, 1.0)
        .a11y_hidden();
    let link = row()
        .key("open-live")
        .px(ROW_PX)
        .gap(6.0)
        .radius(ROW_RADIUS)
        .role("link")
        .label(tr_fill("Open {} in Hex", &shown.title))
        .cursor(Cursor::Pointer)
        .focus_ring(2., -2., p.k.c.border_focus)
        // The surface steps the foot's presence; nothing eases it twice.
        .transition(Transition::none())
        .hover(p.k.c.hover)
        .hover_group();
    let link = if shows_transcript(capsule) {
        // The words keep their place at the bottom line; older ones leave
        // through the fade as new ones arrive.
        let words = col()
            .key("peek")
            .grow(1.0)
            .min_w(0.0)
            .h(LINES as f32 * LINE)
            .justify_end()
            .clip_fade_linear(0.0, FADE, 0.0, 0.0)
            .child(
                text(tail(&capsule.live.entries))
                    .key("peek-words")
                    .w_full()
                    .font(p.description())
                    .color(p.muted())
                    .wrap()
                    .a11y_hidden(),
            );
        // The cue sits on the newest line, which is always there.
        link.h(PEEK_HEIGHT)
            .py(PEEK_PY)
            .items_end()
            .child(words)
            .child(cue.mb(1.0))
    } else {
        let label = text(tr("Open in Hex").to_owned())
            .key("label")
            .font(p.row_title())
            .color(p.muted())
            .truncate()
            .a11y_hidden();
        link.h(LINK_HEIGHT).center_y().child(label).child(cue)
    };
    pressed(link, |c, _| {
        if let (Some(open), State::Recording { recording, .. }) = (&c.open_meeting, &c.state) {
            open(recording.id.clone());
        }
    })
    .opacity(presence)
}

/// The transcript's newest words: enough of its end to fill the foot's
/// lines, from the start of a word.
fn tail(entries: &[TranscriptEntry]) -> String {
    let mut turns = Vec::new();
    let mut chars = 0;
    for entry in entries.iter().rev() {
        let said = entry.text.trim();
        if said.is_empty() {
            continue;
        }
        chars += said.chars().count() + 1;
        turns.push(said);
        if chars > TAIL_CHARS {
            break;
        }
    }
    turns.reverse();
    let words = turns.join(" ");
    let Some((cut, _)) = words.char_indices().rev().nth(TAIL_CHARS) else {
        return words;
    };
    let end = &words[cut..];
    // A language written without spaces is cut between characters.
    let start = end
        .find(char::is_whitespace)
        .unwrap_or_else(|| end.chars().next().map_or(0, char::len_utf8));
    end[start..].trim_start().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(text: &str) -> TranscriptEntry {
        TranscriptEntry {
            source: meeting::MeetingSource::System,
            start_ms: 0,
            end_ms: 1_000,
            text: text.into(),
            speaker: None,
        }
    }

    #[test]
    fn a_short_transcript_is_shown_whole() {
        let entries = [
            said(" Kickoff is at two thirty. "),
            said(""),
            said("Works."),
        ];
        assert_eq!(tail(&entries), "Kickoff is at two thirty. Works.");
        assert_eq!(tail(&[]), "");
    }

    #[test]
    fn a_long_transcript_keeps_its_newest_words_from_a_words_start() {
        let entries: Vec<_> = (0..40)
            .map(|turn| said(&format!("Turn number {turn} said something here.")))
            .collect();
        let tail = tail(&entries);
        assert!(
            tail.ends_with("Turn number 39 said something here."),
            "{tail}"
        );
        assert!(
            tail.chars().count() <= TAIL_CHARS,
            "{}",
            tail.chars().count()
        );
        // It starts at a word, never inside one.
        let first = tail.split(' ').next().unwrap();
        assert!(
            ["Turn", "number", "said", "something", "here."].contains(&first)
                || first.parse::<u32>().is_ok(),
            "{tail}"
        );
        // One turn longer than the whole tail is still cut to it.
        let long = tail_of_one(&"word ".repeat(200));
        assert!(long.chars().count() <= TAIL_CHARS && long.starts_with("word"));
    }

    fn tail_of_one(text: &str) -> String {
        tail(&[said(text)])
    }

    #[test]
    fn the_foot_is_the_plain_link_until_the_call_has_words() {
        assert_eq!(LINK_FOOT, 38.0);
        assert_eq!(PEEK_FOOT, 66.0);
    }
}
