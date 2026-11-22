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

// MARK: - Model lookups

/// The model's sizes, its language and its alphabet's size.
typedef struct {
    size_t word_count;
    size_t pair_count;
    size_t byte_count;
    /// The language it was built for ("en"), NUL-terminated.
    char language[8];
    /// Letter codes plus one: code 0 is unused, letters are 1 to code_count - 1.
    size_t code_count;
    /// The apostrophe's letter code, or -1 when the model has none.
    int32_t apostrophe_code;
} HkModelInfo;

/// Returns 0 and fills `out`, else nonzero.
int32_t hk_model_info(const HkModel *model, HkModelInfo *out);
/// Fills `characters[code]` (a Unicode scalar) and `base_codes[code]` (the unaccented
/// letter's code, its own when it has no accent) for every code below code_count; both hold
/// `capacity` >= code_count items. Returns 0, else nonzero.
int32_t hk_model_alphabet(const HkModel *model, uint32_t *characters, uint8_t *base_codes,
                          size_t capacity);
/// Word `word`'s letter codes, read in place: valid while the model is open. NULL (and
/// `*length` 0) past the last word.
const uint8_t *hk_model_word_codes(const HkModel *model, size_t word, size_t *length);
/// The word's letter codes (composed first, as Swift compares characters) into `out`, which
/// holds `capacity` bytes (the word's UTF-8 length is always enough): their count, -1 when
/// it has a letter the model lacks, -2 on a bad argument or too small an `out`.
int64_t hk_model_encode(const HkModel *model, const char *word, uint8_t *out, size_t capacity);
/// The word's index, or -1 when the model lacks it.
int64_t hk_model_index(const HkModel *model, const char *word);
int64_t hk_model_index_of_codes(const HkModel *model, const uint8_t *codes, size_t count);
/// The words in lo..<hi (sharing their first `depth` codes) whose next code is `code`, as
/// `*out_lo..<*out_hi`; empty when lo..<hi isn't a range of words.
void hk_model_narrow(const HkModel *model, size_t lo, size_t hi, size_t depth, int32_t code,
                     size_t *out_lo, size_t *out_hi);
/// -ln P(word) in nats; NaN past the last word.
double hk_model_unigram_cost(const HkModel *model, size_t word);
/// Returns 1 and writes -ln P(second | first) when the model has the pair, else 0.
int32_t hk_model_bigram_cost(const HkModel *model, size_t first, size_t second, double *out);
/// Returns 1 and writes -ln P(word | previous) with the decoder's 0.92-nat backoff to the
/// unigram (`previous` NULL: the unigram's), or 0 when the model lacks the word.
int32_t hk_model_cost(const HkModel *model, const char *word, const char *previous, double *out);
/// 1 when the model has `second` following `first` as a pair, else 0.
int32_t hk_model_has_pair(const HkModel *model, const char *first, const char *second);

/// Whether the host may offer `word` (an index): nonzero to allow. Called only for words that
/// would make the list, best first, so it runs a handful of times, synchronously, on the
/// calling thread.
typedef uint8_t (*HkAllowWord)(void *context, uint32_t word);

/// The three likeliest allowed words after `previous`, by index into `out` (`capacity` >= 3);
/// returns their count. `allowing` NULL allows every word.
size_t hk_model_next_words(const HkModel *model, const char *previous, HkAllowWord allowing,
                           void *context, uint32_t *out, size_t capacity);
/// The two likeliest allowed words completing a one- to three-letter `prefix`, after
/// `previous` (NULL: none), by index into `out` (`capacity` >= 2); returns their count.
size_t hk_model_completions(const HkModel *model, const char *prefix, const char *previous,
                            HkAllowWord allowing, void *context, uint32_t *out, size_t capacity);
/// How much of the word list's frequency continues the `count` codes of `prefix` with each
/// letter: returns 1 and fills `mass[code]` for every code (`capacity` >= code_count), or 0
/// when no word starts with `prefix`, or one ends there and none goes on.
int32_t hk_model_continuations(const HkModel *model, const uint8_t *prefix, size_t count,
                               double *mass, size_t capacity);

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

// MARK: - Tap decoder (Spatial autocorrect)

/// TypingDecoder.Parameters, field for field. Fill with hk_tap_default_parameters or
/// hk_tap_tuned_parameters.
typedef struct {
    double sigma_x;
    double sigma_y;
    double lm_weight;
    double deletion_cost;
    double insertion_cost;
    double transposition_cost;
    double space_omission_cost;
    double space_tap_cost;
    double space_tap_weight;
    double merge_reach;
    size_t max_edits;
    size_t beam;
    double auto_margin;
    double valid_margin;
    double unknown_valid_cost;
    double unknown_cost;
    double max_tap_cost;
    uint8_t splits;
    double accent_cost;
    double one_letter_word_cost;
    double user_word_cost;
    double accent_margin;
} HkTapParameters;

void hk_tap_default_parameters(HkTapParameters *out);
/// The settings tuned for a language's model ("en", "es"; anything else the defaults).
void hk_tap_tuned_parameters(const char *language, HkTapParameters *out);

typedef struct {
    double x;
    double y;
    double width;
    double height;
} HkRect;

/// The letter keys taps are read against.
typedef struct {
    HkKeys keys;
    /// The bottom letter row's centre, below which a tap may have been meant for space.
    double bottom_row_y;
    /// Nonzero when `space` is the space bar's keycap (both bars on a split keyboard, as the
    /// unsplit keyboard has them).
    uint8_t has_space;
    HkRect space;
} HkTapKeys;

typedef struct HkTapDecoder HkTapDecoder;

/// `user_words` are your own words as you spell them (in order: of two with the same letters,
/// the first counts); `never_words` and `never_pairs` (`never_pair_count` (first, second)
/// pairs, interleaved) are model indices never offered. The decoder keeps its own reference
/// to `model`.
HkTapDecoder *hk_tap_new(const HkModel *model, const HkTapParameters *parameters,
                         const char *const *user_words, size_t user_word_count,
                         const uint32_t *never_words, size_t never_word_count,
                         const uint32_t *never_pairs, size_t never_pair_count);
void hk_tap_free(HkTapDecoder *decoder);

typedef struct {
    /// Nonzero when the decoder could read the taps; zero (and the rest empty) when not.
    uint8_t decided;
    /// What space should put in place of what you typed, or NULL to keep it.
    char *replacement;
    /// Readings for the suggestion strip, best first.
    HkCandidate *readings;
    size_t reading_count;
    double literal_cost;
} HkTapDecision;

/// Reads the word you typed on space. `taps` holds `tap_count` (x, y) pairs, interleaved, one
/// per letter of `typed` (the lowercase letters the keys resolved), in the keys'
/// coordinates; `previous` is NULL or the word before. Returns 0 and fills `out` (release it
/// with hk_tap_decision_free), else nonzero with `out` empty.
int32_t hk_tap_decide(const HkTapDecoder *decoder, const double *taps, size_t tap_count,
                      const char *typed, const char *previous, uint8_t literal_is_valid,
                      const HkTapKeys *keys, HkTapDecision *out);
void hk_tap_decision_free(HkTapDecision *decision);

/// One word where a space was tapped for a letter on the row above ("deli ate" for
/// "delicate"): returns 0 and writes the word to `*out` (release it with hk_string_free), or
/// NULL to keep the two; nonzero on a bad argument.
int32_t hk_tap_merge(const HkTapDecoder *decoder, const char *first, const double *first_taps,
                     size_t first_tap_count, double space_x, double space_y, const char *second,
                     const double *second_taps, size_t second_tap_count, const char *previous,
                     uint8_t first_is_valid, uint8_t second_is_valid, const HkTapKeys *keys,
                     char **out);
void hk_string_free(char *text);

/// Whether hk_tap_decide would put `reading` (one word) in place of what you typed: returns
/// 0 and writes 1 (yes), 0 (no) or -1 (a two-word reading, which only decide can tell) to
/// `*out`; nonzero on a bad argument.
int32_t hk_tap_replaces(const HkTapDecoder *decoder, const char *reading, const double *taps,
                        size_t tap_count, const char *typed, const char *previous,
                        uint8_t literal_is_valid, const HkTapKeys *keys, int32_t *out);

// MARK: - Letter keys from the host's layout

/// A key of the letters page: what it types (NULL: nothing), whether it's a space bar, and
/// its keycap.
typedef struct {
    const char *character;
    uint8_t space;
    HkRect frame;
} HkLayoutKey;

/// Learn how I type's tap count and summed offsets (in keycap widths and heights) for a key;
/// `present` zero when there are none.
typedef struct {
    uint8_t present;
    uint64_t count;
    double dx;
    double dy;
} HkKeyStats;

/// Where you tend to tap a letter, in points from its key's centre.
typedef struct {
    uint32_t letter;
    double dx;
    double dy;
} HkKeyOffset;

/// The offsets of letters with 20 taps or more, shrunk toward the centre while there are
/// few: `stats[i]` is `keys[i]`'s. Writes them to `out` (`capacity` >= key_count is always
/// enough), by letter, and returns their count.
size_t hk_layout_offsets(const HkLayoutKey *keys, const HkKeyStats *stats, size_t key_count,
                         HkKeyOffset *out, size_t capacity);
/// The decoder's keys for a letters page: `split` NULL, or (middle, shift) of a split layout
/// whose halves are pushed together; `pitch_*` the layout's key pitch; `offsets` move letter
/// centres. Writes the centres to `centres` (`capacity` >= key_count is always enough) and
/// `out` (whose `keys.centres` points there). Returns 0, else nonzero.
int32_t hk_layout_tap_keys(const HkLayoutKey *keys, size_t key_count, const double *split,
                           double pitch_width, double pitch_height, const HkKeyOffset *offsets,
                           size_t offset_count, HkKeyCentre *centres, size_t capacity,
                           HkTapKeys *out);

#ifdef __cplusplus
}
#endif

#endif
