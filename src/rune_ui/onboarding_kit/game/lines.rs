//! The practice game's messages: themes of three from one corner of the
//! player's life, each a little worse than the one before, the third the
//! most unhinged. A run plays the themes in a random order; the story plays
//! them as written, so its scripted answers land where they always do.

use super::super::cards::{ADAM, App, DAX, Notice};
use super::super::fixture::Take;

/// One message, and the story's answer to it.
#[derive(Clone, Copy)]
pub(super) struct Line {
    pub(super) notice: Notice,
    pub(super) demo: &'static Take,
    /// How hard the answer's words fly (`Flight` intensity).
    pub(super) heat: f32,
}

/// Three messages, mundane to absurd.
pub(super) type Theme = [Line; 3];

const fn messages(key: &'static str, from: &'static str, body: &'static str) -> Notice {
    Notice {
        key,
        app: App::Messages,
        from,
        place: None,
        body,
        field: "iMessage",
    }
}

const fn slack(
    key: &'static str,
    from: &'static str,
    place: &'static str,
    body: &'static str,
) -> Notice {
    Notice {
        key,
        app: App::Slack,
        from,
        place: Some(place),
        body,
        field: "Reply in thread",
    }
}

const fn line(notice: Notice, demo: &'static Take, heat: f32) -> Line {
    Line { notice, demo, heat }
}

pub(super) const THEMES: [Theme; 9] = [
    // The neighbor.
    [
        line(
            messages(
                "milk",
                "Sam next door",
                "are you near the store? could you grab some milk",
            ),
            &Take {
                updates: &[
                    (0.25, "Sure,"),
                    (0.45, "Sure, oat"),
                    (0.62, "Sure, oat or"),
                    (0.85, "Sure, oat or normal?"),
                    (1.05, "Sure, oat or normal? Back"),
                    (1.2, "Sure, oat or normal? Back in"),
                    (1.42, "Sure, oat or normal? Back in ten."),
                ],
                release: 1.6,
            },
            0.5,
        ),
        line(
            messages(
                "eggs",
                "Sam next door",
                "also eggs. and a ladder. don't ask",
            ),
            &Take {
                updates: &[(0.3, "Asking"), (0.55, "Asking anyway.")],
                release: 0.8,
            },
            0.65,
        ),
        line(
            messages(
                "goose",
                "Sam next door",
                "fine. the goose is on the roof again",
            ),
            &Take {
                updates: &[
                    (0.3, "Tell"),
                    (0.5, "Tell the goose"),
                    (0.8, "Tell the goose I'm coming."),
                ],
                release: 1.05,
            },
            0.92,
        ),
    ],
    // The work Slack spiral.
    [
        line(
            slack(
                "sync",
                "Priya",
                "#general",
                "can we sync for 5 mins before standup?",
            ),
            &Take {
                updates: &[(0.3, "Sure,"), (0.65, "Sure, five minutes.")],
                release: 0.9,
            },
            0.5,
        ),
        line(
            slack(
                "offsite",
                "Priya",
                "#general",
                "update: the 5 min sync is now a two-day offsite",
            ),
            &Take {
                updates: &[(0.3, "I'll pack"), (0.6, "I'll pack a snack.")],
                release: 0.85,
            },
            0.65,
        ),
        line(
            slack(
                "canoe",
                "Priya",
                "#general",
                "the offsite is in a canoe. bring your laptop",
            ),
            &Take {
                updates: &[
                    (0.3, "Booking"),
                    (0.55, "Booking a bigger"),
                    (0.8, "Booking a bigger canoe."),
                ],
                release: 1.0,
            },
            0.9,
        ),
    ],
    // The family group chat.
    [
        line(
            Notice {
                place: Some("Family chat"),
                ..messages("lid", "Mom", "who has the tupperware with the blue lid")
            },
            &Take {
                updates: &[(0.3, "Not me.")],
                release: 0.6,
            },
            0.6,
        ),
        line(
            Notice {
                place: Some("Family chat"),
                ..messages("heirloom", "Mom", "your father says it's an heirloom now")
            },
            &Take {
                updates: &[(0.3, "It's a"), (0.5, "It's a lid.")],
                release: 0.75,
            },
            0.7,
        ),
        line(
            Notice {
                place: Some("Family chat"),
                ..messages(
                    "christmas",
                    "Mom",
                    "christmas is cancelled until the lid comes home",
                )
            },
            &Take {
                updates: &[
                    (0.3, "The lid"),
                    (0.55, "The lid is coming"),
                    (0.8, "The lid is coming home."),
                ],
                release: 1.0,
            },
            0.92,
        ),
    ],
    // The landlord.
    [
        line(
            messages(
                "alarm",
                "Dave · Landlord",
                "stopping by tuesday to check the smoke alarm",
            ),
            &Take {
                updates: &[(0.3, "Tuesday"), (0.55, "Tuesday works.")],
                release: 0.8,
            },
            0.5,
        ),
        line(
            messages(
                "cousin",
                "Dave · Landlord",
                "bringing my cousin. he's sort of a plumber",
            ),
            &Take {
                updates: &[(0.3, "Sort of?")],
                release: 0.6,
            },
            0.8,
        ),
        line(
            messages(
                "boiler",
                "Dave · Landlord",
                "good news, my cousin lives in your boiler now",
            ),
            &Take {
                updates: &[(0.3, "Welcome"), (0.55, "Welcome home, cousin.")],
                release: 0.85,
            },
            0.9,
        ),
    ],
    // The dating app.
    [
        line(
            messages(
                "coffee",
                "Jordan · Hinge",
                "hey! still on for coffee saturday?",
            ),
            &Take {
                updates: &[(0.3, "Can't"), (0.5, "Can't wait.")],
                release: 0.75,
            },
            0.5,
        ),
        line(
            messages(
                "vet",
                "Jordan · Hinge",
                "my mom's coming too. she wants to vet you",
            ),
            &Take {
                updates: &[(0.3, "Tell her"), (0.6, "Tell her I'm lovely.")],
                release: 0.85,
            },
            0.65,
        ),
        line(
            messages(
                "venue",
                "Jordan · Hinge",
                "mom booked a venue. we get engaged at two",
            ),
            &Take {
                updates: &[
                    (0.3, "Can we"),
                    (0.55, "Can we do"),
                    (0.8, "Can we do two thirty?"),
                ],
                release: 1.0,
            },
            0.92,
        ),
    ],
    // The group project.
    [
        line(
            slack("slides", "Tom", "#group-4", "did anyone start the slides?"),
            &Take {
                updates: &[(0.3, "Not yet.")],
                release: 0.6,
            },
            0.55,
        ),
        line(
            slack(
                "one-slide",
                "Tom",
                "#group-4",
                "ok I started. it's one slide. it says hi",
            ),
            &Take {
                updates: &[(0.3, "Strong"), (0.5, "Strong start.")],
                release: 0.75,
            },
            0.7,
        ),
        line(
            slack(
                "airport",
                "Tom",
                "#group-4",
                "we present in 10. I'm at the airport. good luck",
            ),
            &Take {
                updates: &[
                    (0.3, "Safe"),
                    (0.5, "Safe travels,"),
                    (0.75, "Safe travels, Tom."),
                ],
                release: 0.95,
            },
            0.92,
        ),
    ],
    // The pet sitter.
    [
        line(
            messages(
                "judgy",
                "Riley · Pet sitter",
                "fed the cat! she seems a bit judgmental",
            ),
            &Take {
                updates: &[(0.3, "She is.")],
                release: 0.6,
            },
            0.6,
        ),
        line(
            messages(
                "thermostat",
                "Riley · Pet sitter",
                "she found the thermostat. it's 90 degrees",
            ),
            &Take {
                updates: &[(0.3, "She runs"), (0.5, "She runs cold.")],
                release: 0.75,
            },
            0.7,
        ),
        line(
            messages(
                "wifi",
                "Riley · Pet sitter",
                "she changed the wifi password and won't tell me",
            ),
            &Take {
                updates: &[(0.3, "Try"), (0.5, "Try tuna.")],
                release: 0.7,
            },
            0.95,
        ),
    ],
    // Dad's tech support.
    [
        line(
            messages("bigger", "Dad", "how do I make the text bigger"),
            &Take {
                updates: &[(0.3, "Command"), (0.5, "Command plus.")],
                release: 0.75,
            },
            0.5,
        ),
        line(
            messages("huge", "Dad", "the text is huge now. one word per screen"),
            &Take {
                updates: &[(0.3, "Command"), (0.5, "Command minus.")],
                release: 0.75,
            },
            0.65,
        ),
        line(
            messages(
                "fridge",
                "Dad",
                "I pressed something and the fridge is on zoom",
            ),
            &Take {
                updates: &[
                    (0.3, "Wave"),
                    (0.5, "Wave to the"),
                    (0.75, "Wave to the fridge."),
                ],
                release: 0.95,
            },
            0.92,
        ),
    ],
    // The team.
    [
        line(
            DAX,
            &Take {
                updates: &[
                    (0.3, "On"),
                    (0.45, "On it,"),
                    (0.75, "On it, five minutes."),
                ],
                release: 1.0,
            },
            0.6,
        ),
        line(
            ADAM,
            &Take {
                updates: &[
                    (0.3, "Figma"),
                    (0.55, "Figma deserved"),
                    (0.85, "Figma deserved it."),
                ],
                release: 1.05,
            },
            0.75,
        ),
        line(
            slack(
                "review",
                "Frank",
                "#hex",
                "the PR is reviewing us now. I got changes requested",
            ),
            &Take {
                updates: &[(0.3, "Fair,"), (0.5, "Fair, honestly.")],
                release: 0.75,
            },
            0.92,
        ),
    ],
];

/// The order a run plays the themes in, its `cycle`th time through them:
/// seed 0 plays them as written, any other seed shuffles each cycle.
pub(super) fn order(seed: u64, cycle: usize) -> [usize; THEMES.len()] {
    let mut order: [usize; THEMES.len()] = std::array::from_fn(|i| i);
    if seed == 0 {
        return order;
    }
    let mut state = seed ^ (cycle as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for i in (1..order.len()).rev() {
        // splitmix64
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        order.swap(i, (z % (i as u64 + 1)) as usize);
    }
    order
}

/// The `n`th message a run with `seed` shows, counting messages only.
pub(super) fn nth(seed: u64, n: usize) -> Line {
    let per_cycle = THEMES.len() * 3;
    let order = order(seed, n / per_cycle);
    let within = n % per_cycle;
    THEMES[order[within / 3]][within % 3]
}

/// A seed for a real run: different every time, never the story's 0.
pub(super) fn fresh_seed() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.as_nanos() as u64);
    nanos | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_message_fits_one_line_of_the_card() {
        let k = rune_theme_opencode::kit(true);
        let width = super::super::super::cards::CARD_W - 2.0 * 14.0 - 32.0 - 10.0;
        let (_, one) = rune::text::measure("M", &[], k.ty.ui, Some(width));
        let mut keys = std::collections::HashSet::new();
        for line in THEMES.iter().flatten() {
            let n = line.notice;
            let (_, h) = rune::text::measure(n.body, &[], k.ty.ui, Some(width));
            assert_eq!(h, one, "{:?} wraps", n.body);
            assert!(keys.insert(n.key), "{} twice", n.key);
            assert!(!line.demo.text().is_empty());
        }
    }

    #[test]
    fn a_run_plays_every_theme_in_threes_before_any_again() {
        for seed in [0, 1, 7, 12345] {
            let first: Vec<&str> = (0..THEMES.len() * 3)
                .map(|n| nth(seed, n).notice.key)
                .collect();
            let unique: std::collections::HashSet<_> = first.iter().collect();
            assert_eq!(unique.len(), first.len());
            for (theme, three) in first.chunks(3).enumerate() {
                let written = THEMES.iter().find(|t| t[0].notice.key == three[0]);
                let written = written.unwrap_or_else(|| panic!("theme {theme} starts mid-way"));
                let keys: Vec<&str> = written.iter().map(|l| l.notice.key).collect();
                assert_eq!(keys, three);
            }
        }
        assert_eq!(nth(0, 0).notice.key, "milk");
        assert_ne!(order(3, 0), order(3, 1), "each cycle shuffles again");
    }
}
