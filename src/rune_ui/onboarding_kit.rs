//! Pieces for the next onboarding, and the options built from them as
//! playground stories (only with the `playground` experiment; the shipping
//! tour in [`onboarding`](super::onboarding) doesn't use them yet).
//!
//! - [`hex_meter`]: the swappable thin single-hexagon mark (`t573` Hairline)
//!   and calibrating scope loader.
//! - [`pill`]: the download pill and inline language capsule button.
//! - [`cards`]: example macOS notifications to answer, drawn on theme tokens.
//! - [`field_text`]: a reply laid out by hand, for a caret mid-line and
//!   the slots words fly into.
//! - [`words`]: live words with a caret, and shout words.
//! - [`flight`] & [`sparks`]: word flight and crisp landing rings/sparks.
//! - [`keyboard`]: a 3D perspective-tilted Mac keyboard whose keys sink in Z,
//!   and [`typing`], a slow, typo-heavy reply typed on it by hand.
//! - [`permissions`]: calm permission grants and the escaping Accessibility
//!   helper panel that detaches from Hex's card and parks under System Settings.
//! - [`mic`] & [`mic_line`]: the single-hexagon voice scope and microphone
//!   selector capsule.
//! - [`landing`]: shared typography, zero-bounce transitions, and `"Hex is ready"`.

mod cards;
mod field_text;
mod first_run;
mod fixture;
mod flight;
mod flows;
mod hex_meter;
mod keyboard;
mod landing;
mod mic;
mod mic_line;
mod permissions;
mod pill;
mod sparks;
mod stories;
mod typing;
mod versus;
mod words;

pub(crate) use stories::add as stories;
