//! `text.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: nil)`: a port of
//! CoreFoundation's `CFStringFold` (`__CFStringFoldCharacterClusterAtIndex`,
//! swift-corelibs-foundation CFString.c). It isn't Unicode case folding: each scalar folds by
//! CF's own table (iOS's, `unicode_generated.rs`: "ß" → "ss", "ﬁ" → "fi", "Ꭰ" → "ꭰ"), and the
//! marks after a base are dropped only when the folded base is Latin, Greek or Cyrillic (below
//! U+0510); after any other base they stay, canonically decomposed and ordered, except a first
//! mark below U+0510 after a base the fold left unchanged, which goes.

use unicode_normalization::char::{canonical_combining_class, decompose_canonical};

use super::unicode;

pub(super) fn fold(text: &str) -> String {
    let scalars: Vec<char> = text.chars().collect();
    let mut out: Vec<char> = Vec::with_capacity(scalars.len());
    let mut index = 0;
    while index < scalars.len() {
        let c = scalars[index];
        index += 1;
        if c == '\0' {
            out.push(c);
            continue;
        }
        let cluster_start = out.len();
        let changed = unicode::fold_scalar(c).is_some();
        let base = match unicode::fold_scalar(c) {
            Some(folded) => {
                out.extend(folded.chars());
                folded.chars().last().unwrap_or(c)
            }
            None => {
                out.push(c);
                c
            }
        };
        if (base as u32) < 0x0510 {
            while index < scalars.len() && unicode::cf_extend(scalars[index]) {
                index += 1;
            }
            continue;
        }
        // CF drops a first mark below U+0510 only after a base it left as it was.
        let mut first = !changed;
        let mut filled = false;
        while index < scalars.len() && unicode::cf_extend(scalars[index]) {
            let mark = scalars[index];
            index += 1;
            if first && (mark as u32) <= 0x050F {
                first = false;
                continue;
            }
            first = false;
            filled = true;
            decompose_canonical(mark, |d| out.push(d));
        }
        if filled {
            sort_marks(&mut out[cluster_start..]);
        }
    }
    out.into_iter().collect()
}

/// `CFUniCharPrioritySort` from the last scalar before the cluster's final marks (or its
/// start): past the scalars without a combining class, a stable sort by combining class.
fn sort_marks(cluster: &mut [char]) {
    let mut from = cluster.len() - 1;
    while from > 0 && unicode::cf_extend(cluster[from]) {
        from -= 1;
    }
    if cluster.len() - from <= 1 {
        return;
    }
    let tail = &mut cluster[from..];
    let skip = tail
        .iter()
        .take_while(|&&c| canonical_combining_class(c) == 0)
        .count();
    tail[skip..].sort_by_key(|&c| canonical_combining_class(c));
}
