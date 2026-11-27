//! What the tagger is given and what its answer means.
//!
//! The model reads at most 128 tokens at a time, so a transcript is tagged
//! in chunks of whole words. Each chunk is [CLS], its words' WordPiece ids
//! and [SEP], zero-padded to one of the model's enumerated lengths. The
//! model answers one logit per token, and a word's is the one at its first
//! subword.

use super::plan::Proposal;
use super::tokenizer::{CLS, PAD, SEP, Tokenizer};
use super::words::{Span, Words};

/// The longest model input, [CLS] and [SEP] included.
const MAX_TOKENS: usize = 128;
/// The model's enumerated sequence lengths.
const SEQUENCE_LENGTHS: [usize; 4] = [16, 32, 64, 128];

/// One run of the model over some neighbouring words.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Chunk {
    /// [CLS], the words' WordPiece ids, [SEP].
    ids: Vec<i32>,
    /// Each word and where its first subword sits in `ids`. A word with no
    /// tokens sits nowhere.
    words: Vec<(Span, Option<usize>)>,
}

impl Tokenizer {
    /// The model runs that tag `words`, in order. Every word is in exactly
    /// one chunk. A single word longer than a whole input keeps only its
    /// first tokens, since only its first subword is scored.
    pub(crate) fn chunks(&self, words: &Words) -> Vec<Chunk> {
        let tokens = words.iter().map(|word| self.encode(&model_word(word)));
        pack(words, tokens)
    }
}

impl Chunk {
    /// The model's `input_ids`: the chunk padded to the next enumerated
    /// length.
    pub(crate) fn input_ids(&self) -> Vec<i32> {
        let length = SEQUENCE_LENGTHS
            .into_iter()
            .find(|&length| length >= self.ids.len())
            .unwrap_or(self.ids.len());
        let mut ids = self.ids.clone();
        ids.resize(length, PAD);
        ids
    }

    /// The model's `length`: how many of the input ids are real.
    pub(crate) fn length(&self) -> usize {
        self.ids.len()
    }

    /// What the model's `delete_logit` output proposes: each word it labels
    /// DELETE, with P(DELETE) as the confidence. A word with no tokens is
    /// never proposed. Nothing comes of an output shorter than the input.
    pub(crate) fn proposals(&self, logits: &[f32]) -> Option<Vec<Proposal>> {
        if logits.len() < self.ids.len() {
            return None;
        }
        let proposals = self.words.iter().filter_map(|&(word, position)| {
            let probability = 1.0 / (1.0 + (-f64::from(logits[position?])).exp());
            (probability > 0.5).then(|| Proposal::delete(word, probability))
        });
        Some(proposals.collect())
    }
}

/// Whole words per chunk, each chunk within [`MAX_TOKENS`].
fn pack(words: &Words, tokens: impl Iterator<Item = Vec<i32>>) -> Vec<Chunk> {
    let fresh = || Chunk {
        ids: vec![CLS],
        words: Vec::new(),
    };
    let mut chunks = Vec::new();
    let mut current = fresh();
    for (index, mut tokens) in tokens.enumerate() {
        let Some(word) = words.span(index..index + 1) else {
            break;
        };
        if !current.words.is_empty() && current.ids.len() + 1 + tokens.len() > MAX_TOKENS {
            current.ids.push(SEP);
            chunks.push(std::mem::replace(&mut current, fresh()));
        }
        tokens.truncate(MAX_TOKENS - 2);
        current
            .words
            .push((word, (!tokens.is_empty()).then_some(current.ids.len())));
        current.ids.append(&mut tokens);
    }
    if !current.words.is_empty() {
        current.ids.push(SEP);
        chunks.push(current);
    }
    chunks
}

/// The word as the tagger sees it: a hyphen between letters is dropped. The
/// tokenizer splits "Hye-jin" into "hye", "-", "jin", exactly what a stutter
/// fragment and the next word look like ("w- we"), which training always
/// deleted. "Hyejin" reads as one name. Only the model's input changes,
/// never the text.
fn model_word(word: &str) -> String {
    let characters: Vec<char> = word.chars().collect();
    let between_letters = |index: usize| {
        index > 0
            && characters[index - 1].is_alphabetic()
            && characters
                .get(index + 1)
                .is_some_and(|next| next.is_alphabetic())
    };
    characters
        .iter()
        .enumerate()
        .filter(|&(index, &ch)| !(ch == '-' && between_letters(index)))
        .map(|(_, &ch)| ch)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// As many one-letter words as there are token lists.
    fn packed(tokens: Vec<Vec<i32>>) -> Vec<Chunk> {
        let words = Words::new(vec!["w"; tokens.len()].join(" "));
        pack(&words, tokens.into_iter())
    }

    /// The iOS suite's own cases (`testChunksKeepWholeWordsWithinTheModelLength`).
    #[test]
    fn chunks_keep_whole_words_within_the_model_length() {
        let chunks = packed(vec![vec![7, 8], vec![], vec![9]]);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].ids, [101, 7, 8, 9, 102]);
        let positions: Vec<_> = chunks[0]
            .words
            .iter()
            .map(|&(_, position)| position)
            .collect();
        assert_eq!(positions, [Some(1), None, Some(3)]);

        let chunks = packed(vec![vec![5, 6]; 70]);
        let sizes: Vec<_> = chunks.iter().map(|chunk| chunk.words.len()).collect();
        assert_eq!(sizes, [63, 7]);
        assert!(chunks.iter().all(|chunk| {
            chunk.length() <= MAX_TOKENS
                && chunk.ids.first() == Some(&CLS)
                && chunk.ids.last() == Some(&SEP)
        }));
        let covered: Vec<_> = chunks
            .iter()
            .flat_map(|chunk| chunk.words.iter().flat_map(|(word, _)| word.range()))
            .collect();
        assert_eq!(covered, (0..70).collect::<Vec<_>>());

        let long = packed(vec![vec![1], vec![4; 200]]);
        let lengths: Vec<_> = long.iter().map(Chunk::length).collect();
        assert_eq!(lengths, [3, MAX_TOKENS]);
    }

    #[test]
    fn a_chunk_is_padded_to_the_next_length_the_model_takes() {
        let short = &packed(vec![vec![7, 8], vec![9]])[0];
        assert_eq!(short.length(), 5);
        assert_eq!(short.input_ids().len(), 16);
        assert_eq!(short.input_ids()[..6], [101, 7, 8, 9, 102, PAD]);

        for (words, padded) in [(14, 16), (15, 32), (62, 64), (63, 128), (126, 128)] {
            let chunk = &packed(vec![vec![5]; words])[0];
            assert_eq!(chunk.length(), words + 2);
            assert_eq!(chunk.input_ids().len(), padded);
        }
        assert!(packed(Vec::new()).is_empty());
    }

    #[test]
    fn a_logit_proposes_its_word_only_when_the_model_labels_it_delete() {
        let words = Words::new("um yes uh");
        let chunk = &pack(&words, [vec![7, 8], vec![], vec![9]].into_iter())[0];
        // [CLS] 7 8 9 [SEP]: "um" is scored at 1, "uh" at 3, "yes" nowhere.
        let proposals = chunk.proposals(&[9.0, 4.0, 9.0, -0.5, 9.0]).unwrap();
        assert_eq!(
            proposals,
            [Proposal::delete(
                words.span(0..1).unwrap(),
                1.0 / (1.0 + (-4.0_f64).exp())
            )]
        );
        let both = chunk
            .proposals(&[0.0, 0.1, 0.0, 12.0, 0.0, 0.0, 0.0])
            .unwrap();
        assert_eq!(both.len(), 2);
        assert!(both[1].confidence() > 0.999);
        assert_eq!(chunk.proposals(&[0.0; 5]).unwrap(), []);
        assert_eq!(chunk.proposals(&[9.0; 4]), None);
    }

    #[test]
    fn the_model_reads_a_hyphenated_name_as_one_word() {
        assert_eq!(model_word("Hye-jin"), "Hyejin");
        assert_eq!(model_word("follow-up,"), "followup,");
        assert_eq!(model_word("w-"), "w-");
        assert_eq!(model_word("-5"), "-5");
        assert_eq!(model_word("a-1"), "a-1");
        assert_eq!(model_word("x--y"), "x--y");
        assert_eq!(model_word("plain"), "plain");
    }
}
