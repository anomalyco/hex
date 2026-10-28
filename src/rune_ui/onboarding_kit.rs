//! Pieces for the next onboarding, and the options built from them as
//! playground stories (only with the `playground` experiment; the shipping
//! tour in [`onboarding`](super::onboarding) doesn't use them yet).
//!
//! - [`hex_meter`]: the Hex mark as a progress meter, inner rings first.
//! - [`pill`]: the download pill, a shared element that flies where it's
//!   placed next.
//! - [`cards`]: example notifications to answer by voice.
//! - [`words`]: live words with a caret, and shout words.
//! - [`mic`]: the microphone's level.
//! - [`landing`]: the language pick and the Ready landing.
//! - [`mic_slot`]: where the microphone picker (t421) goes on the try
//!   screen, held until that piece lands.
//!
//! Each reads only what the running app already reports (see
//! [`fixture`]): download bytes from `transcription_preparation`, the
//! HUD's meter, and partial transcripts from Apple Speech on macOS 26 or
//! a streaming model once it's downloaded.

mod cards;
mod fixture;
mod flows;
mod hex_meter;
mod landing;
mod mic;
mod mic_slot;
mod pill;
mod stories;
mod words;

pub(crate) use stories::add as stories;
