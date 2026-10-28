//! Pieces for the next onboarding, and the options built from them as
//! playground stories (only with the `playground` experiment; the shipping
//! tour in [`onboarding`](super::onboarding) doesn't use them yet).
//!
//! - [`hex_meter`]: the Hex mark as a progress meter, inner rings first.
//! - [`pill`]: the download pill, a shared element that flies where it's
//!   placed next.
//! - [`cards`]: example notifications to answer, drawn as macOS draws
//!   them, stacked as Notification Center stacks them.
//! - [`field_text`]: a reply laid out by hand, for a caret mid-line and
//!   the slots words fly into.
//! - [`words`]: live words with a caret, and shout words.
//! - [`flight`]: the site hero's word flight, ported.
//! - [`keyboard`]: a Mac keyboard whose keys sink as they're pressed, and
//!   [`typing`], a reply typed on it by hand, typos and all.
//! - [`mic`]: the microphone's level.
//! - [`landing`]: the language pick and the Ready landing.
//! - [`mic_line`]: the current tour's microphone picker (t421) on the try
//!   screen.
//!
//! Each reads only what the running app already reports (see
//! [`fixture`]): download bytes from `transcription_preparation`, the
//! HUD's meter, and partial transcripts from Apple Speech on macOS 26 or
//! a streaming model once it's downloaded.

mod cards;
mod field_text;
mod fixture;
mod flight;
mod flows;
mod hex_meter;
mod keyboard;
mod landing;
mod mic;
mod mic_line;
mod pill;
mod stories;
mod typing;
mod versus;
mod words;

pub(crate) use stories::add as stories;
