// The Hex keyboard core's C interface (crates/keyboard/ffi). Hand-written; keep it in step
// with src/lib.rs. Every function catches Rust panics: a failure returns NULL or a nonzero
// status, never unwinds into the caller.
//
// Ownership: each *_open / *_new is released by its *_free. Strings in are UTF-8 and
// NUL-terminated; strings out belong to the struct that holds them and go with its *_free.
// Handles are immutable once made and may be read from any thread at once.

#ifndef HEX_KEYBOARD_H
#define HEX_KEYBOARD_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/// Bumped whenever a signature or struct below changes.
#define HK_ABI_VERSION 2

uint32_t hk_abi_version(void);

// MARK: - Typing model (HXTM v3)

typedef struct HkModel HkModel;

/// Maps a model file read-only. NULL when it can't be opened or isn't HXTM v3.
HkModel *hk_model_open(const char *path);
/// Maps `length` bytes at `offset` of an open file (an uncompressed APK asset). The caller may
/// close `fd` afterwards.
HkModel *hk_model_open_fd(int fd, int64_t offset, int64_t length);
void hk_model_free(HkModel *model);
size_t hk_model_word_count(const HkModel *model);

// MARK: - Swipe typing

/// SwipeDecoder.Parameters, field for field. Fill with hk_swipe_default_parameters.
typedef struct {
    size_t samples;
    double start_radius;
    double end_radius;
    size_t max_start_keys;
    size_t max_end_keys;
    double near_radius;
    double min_length_ratio;
    double max_length_ratio;
    double start_sigma;
    double end_sigma;
    double start_stray_sigma;
    double end_stray_sigma;
    double stray;
    double location_weight;
    double shape_weight;
    double lm_weight;
    size_t shortlist;
    size_t prefilter;
    double band;
    double minimum_length;
    double contraction_missing_pair;
    double evidence_step;
    double turn_window;
    double turn_from;
    double turn_full;
    double dwell_window;
    double dwell_from;
    double dwell_full;
    double evidence_floor;
    double evidence_spacing;
    double evidence_guard;
    double evidence_sigma;
    double evidence_reach;
    double unexplained_cost;
    double straight_key_cost;
    double corner_key_cost;
    double straight_angle;
} HkSwipeParameters;

void hk_swipe_default_parameters(HkSwipeParameters *out);

/// The cost (-ln P) of a common English contraction ("isn't", "we're"): 7.5.
double hk_swipe_contraction_cost(void);
/// The cost of one of your own words: 11.
double hk_swipe_own_word_cost(void);
/// The cost of English contraction `word` (any case) by how often a chat uses it: the
/// commonest ("I'm", "don't") `base` - 1.5, common ones `base`, the rest `base` + 2. NULL or
/// not UTF-8 is none of them: `base` + 2.
double hk_swipe_cost_of_contraction(const char *word, double base);

/// A word read beside the model's: an English contraction or one of your own words.
typedef struct {
    const char *text;
    double cost;
    /// Nonzero when its first word ("i" for "I'm") weighs it after the word before.
    uint8_t contraction;
} HkExtraWord;

/// A letter key: its character (one Unicode scalar) and centre.
typedef struct {
    uint32_t letter;
    double x;
    double y;
} HkKeyCentre;

typedef struct {
    const HkKeyCentre *centres;
    size_t centre_count;
    /// Key width plus gap across; row height plus gap down.
    double pitch_width;
    double pitch_height;
} HkKeys;

typedef struct HkSwipeDecoder HkSwipeDecoder;

/// `never_words` are model word indices never read; `never_pairs` holds `never_pair_count`
/// (first, second) index pairs, interleaved. `extra` in order: a repeat is left out. The
/// decoder keeps its own reference to `model`.
HkSwipeDecoder *hk_swipe_new(const HkModel *model, const HkSwipeParameters *parameters,
                             const uint32_t *never_words, size_t never_word_count,
                             const uint32_t *never_pairs, size_t never_pair_count,
                             const HkExtraWord *extra, size_t extra_count);
void hk_swipe_free(HkSwipeDecoder *decoder);

typedef struct {
    char *text;
    double cost;
} HkCandidate;

typedef struct {
    HkCandidate *candidates;
    size_t candidate_count;
    /// Words that passed the key filters and were scored.
    size_t scored;
} HkSwipeReading;

/// Reads a swipe. `points` holds `point_count` (x, y) pairs, interleaved, in the keys'
/// coordinates; `times` is NULL or `point_count` seconds; `previous` is NULL or the word
/// before. Returns 0 and fills `out` (release it with hk_swipe_reading_free), else nonzero
/// with `out` empty.
int32_t hk_swipe_read(const HkSwipeDecoder *decoder, const double *points, size_t point_count,
                      const double *times, const HkKeys *keys, const char *previous, size_t limit,
                      HkSwipeReading *out);
void hk_swipe_reading_free(HkSwipeReading *reading);

// MARK: - What a swipe types

/// Whether the host's dictionary knows `word` (iOS: UITextChecker): nonzero when it does.
/// `context` is the caller's, passed back as given. Called only during the call it's passed to.
typedef uint8_t (*HkIsWordValid)(void *context, const char *word);

/// A text replacement: its shortcut (lowercased) and what it puts in.
typedef struct {
    const char *shortcut;
    const char *replacement;
} HkTextReplacement;

/// Your text replacements and words, as autocorrect has them. Immutable: make a new one
/// whenever one of them changes.
typedef struct HkSwipeWords HkSwipeWords;

/// `language` is the layout's ("en", "de"…); `has_dictionary` nonzero when the host has one for
/// it. `own` are your other own spellings (lexicon terms, words you kept, fixes Hex learned),
/// `vocabulary` your vocabulary's words as you added them. NULL when an argument is malformed.
HkSwipeWords *hk_swipe_words_new(const char *language, uint8_t has_dictionary,
                                 const HkTextReplacement *replacements, size_t replacement_count,
                                 const char *const *own, size_t own_count,
                                 const char *const *vocabulary, size_t vocabulary_count);
void hk_swipe_words_free(HkSwipeWords *words);

/// The order of a reading's candidates (`texts` and `costs`, `count` of each, best first) once
/// a rare name or abbreviation costs 4 more: fills `order` with `count` indices. Returns 0, else
/// nonzero with `order` untouched.
int32_t hk_swipe_rank(const HkSwipeDecoder *decoder, const HkSwipeWords *words,
                      const char *const *texts, const double *costs, size_t count,
                      HkIsWordValid is_word_valid, void *context, size_t *order);

/// Shift as the keys show it.
enum { HK_SHIFT_OFF = 0, HK_SHIFT_ON = 1, HK_SHIFT_LOCKED = 2 };
/// The field's auto-capitalisation; NONE also when Settings › Auto-capitalization is off.
enum {
    HK_AUTOCAPITALIZATION_NONE = 0,
    HK_AUTOCAPITALIZATION_WORDS = 1,
    HK_AUTOCAPITALIZATION_SENTENCES = 2,
    HK_AUTOCAPITALIZATION_ALL_CHARACTERS = 3,
};

/// The state a swipe is typed in.
typedef struct {
    /// The text before the cursor as Hex expects it, or NULL when the host reports none.
    const char *before;
    uint8_t shift;
    uint8_t shift_held;
    uint8_t auto_capitalization;
    /// Settings › Auto-capitalization.
    uint8_t auto_capitalizes;
    /// Nonzero when the field allows autocorrection (text replacements show only then).
    uint8_t autocorrection;
} HkSwipeContext;

/// What a finished swipe types.
typedef struct {
    /// Nonzero when a space goes in before the word.
    uint8_t space;
    char *word;
    /// The strip's other readings, cased as the word is, best first (at most three).
    char **alternatives;
    size_t alternative_count;
} HkSwipeTyped;

/// What a swipe whose readings are `readings` (`reading_count` ≥ 1, best first, those that may
/// be offered) types in `context`. Returns 0 and fills `out` (release it with
/// hk_swipe_typed_free), else nonzero with `out` empty.
int32_t hk_swipe_typed(const HkSwipeWords *words, const char *const *readings, size_t reading_count,
                       const HkSwipeContext *context, HkIsWordValid is_word_valid, void *callback_context,
                       HkSwipeTyped *out);
void hk_swipe_typed_free(HkSwipeTyped *typed);

/// Nonzero when a swiped word needs a space before it after `before` (NULL: none reported).
uint8_t hk_swipe_needs_space(const char *before);

/// Nonzero when a swipe through `points` (`point_count` (x, y) pairs, interleaved, in points)
/// was a tap that slid: it never went more than 1.6 × `pitch` from where it went down, it
/// lifted on the key it went down on (`key`, that key's character; NULL when it lifted
/// elsewhere), and `first_reading` (its best reading, or NULL) doesn't start and end on that key.
uint8_t hk_swipe_is_slid_tap(const double *points, size_t point_count, double pitch,
                             const char *key, const char *first_reading);

#ifdef __cplusplus
}
#endif

#endif
