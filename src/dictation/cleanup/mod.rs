//! Dictation cleanup: typed edits over the words that were heard.
//!
//! Cleanup is one mechanism. The transcript is split once into [`Words`]
//! and never changed. A proposer, such as the tagger, makes [`Proposal`]s
//! to remove some of them, the [`CleanupGuard`] admits or refuses each one
//! and says why, and [`render`] writes the text the admitted [`EditPlan`]
//! leaves. No edit carries text, so a cleanup can drop a word and never
//! write one: it cannot answer a question, follow a dictated instruction
//! or invent anything.
//!
//! Everything here is pure, with no model, clock, setting or file, so it
//! runs against the iOS app's own fixtures: the tokenizer against Hugging
//! Face's ids, the renderer against the tagger's JavaScript reference, and
//! the guard against what the iOS guard decides for the same words
//! (`parity_tests.rs`).
//!
//! The pieces a model run needs are here too: a [`Tokenizer`] turns words
//! into the [`Chunk`]s the tagger reads, and a chunk turns the tagger's
//! answer back into proposals.
//!
//! Labs › Clean up what I say runs all of it on macOS ([`stage`]): the
//! tagger is the iPhone app's 28 MB model, fetched while the lab is on
//! (`model`, `archive`) and kept warm on the CPU through Core ML
//! (`tagger`). No other OS runs a model yet.
#![cfg_attr(
    not(test),
    expect(
        dead_code,
        unused_imports,
        reason = "nothing reads a plan's edits or the guard's refusals yet, and only macOS runs the tagger"
    )
)]

#[cfg(any(target_os = "macos", test))]
mod archive;
mod chunk;
mod guard;
#[cfg(any(target_os = "macos", test))]
mod model;
mod plan;
mod render;
mod stage;
#[cfg(any(target_os = "macos", test))]
mod tagger;
mod tokenizer;
mod words;

pub(crate) use chunk::Chunk;
pub(crate) use guard::{
    Admitted, CleanupGuard, Policy, ProtectedTerms, Protection, Reason, Refused,
};
pub(crate) use plan::{Disfluency, Edit, EditOp, EditPlan, Proposal};
pub(crate) use render::render;
#[cfg(target_os = "macos")]
pub(crate) use stage::prepare;
pub(crate) use stage::{Dictation, reconcile, stage};
#[cfg(test)]
pub(crate) use stage::{Skip, stand_in};
pub(crate) use tokenizer::{Tokenizer, WrongVocabularySize};
pub(crate) use words::{Span, Words};

#[cfg(test)]
mod parity_tests;
#[cfg(test)]
mod property_tests;
#[cfg(all(test, target_os = "macos"))]
mod real_model_tests;
