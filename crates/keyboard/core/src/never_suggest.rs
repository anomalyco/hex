//! Model words and two-word expressions never offered (`OffensiveWords`, from
//! `never-suggest.tsv`): still scored as words, so what you typed is read right, but never a
//! reading or a replacement. Ported from `TypingDecoder.NeverSuggest`.

use std::collections::HashSet;

use crate::model::Model;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NeverSuggest {
    pub words: HashSet<usize>,
    /// (first word, second word) model indices.
    pub pairs: HashSet<(usize, usize)>,
}

impl NeverSuggest {
    /// `blocked`'s words and two-word phrases that are in `model`.
    pub fn new<'a>(blocked: impl IntoIterator<Item = &'a str>, model: &Model) -> Self {
        let mut out = Self::default();
        for entry in blocked {
            let parts: Vec<&str> = entry.split(' ').filter(|p| !p.is_empty()).collect();
            match parts.as_slice() {
                [_] => {
                    if let Some(i) = model.index(entry) {
                        out.words.insert(i);
                    }
                }
                [a, b] => {
                    if let (Some(a), Some(b)) = (model.index(a), model.index(b)) {
                        out.pairs.insert((a, b));
                    }
                }
                _ => {}
            }
        }
        out
    }

    /// Whether `word` after `previous` makes a blocked two-word expression.
    pub fn follows(&self, word: usize, previous: Option<usize>) -> bool {
        match previous {
            Some(previous) if !self.pairs.is_empty() => self.pairs.contains(&(previous, word)),
            _ => false,
        }
    }
}
