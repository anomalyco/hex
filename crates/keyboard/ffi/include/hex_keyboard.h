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

/// Bumped whenever a signature or struct below changes. 2 is the tap decoder's (PR 2); 3 adds
/// the dictation rules (hk_dictation_*).
#define HK_ABI_VERSION 3

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

// MARK: - Dictation's text rules

// Text goes both ways as UTF-8 bytes and a length, never NUL-terminated, so a transcript
// holding U+0000 survives. Each hk_dictation_* call writes its result to `out` and returns one
// of these; on any other status `out` is left empty. Text the core writes is released with
// hk_text_free (hk_text_list_free for a list).

/// `out` holds the result.
#define HK_DICTATION_OK 0
/// A NULL pointer where one is needed, text that isn't UTF-8, or an unknown style or convention.
#define HK_DICTATION_INVALID 1
/// The core failed (a panic, caught).
#define HK_DICTATION_FAILED 2
/// hk_dictation_spoken_symbols only: two addresses said back to back ("a at sign b.com at sign
/// c.com"), where the Swift original trapped. What to write there is still to be decided.
#define HK_DICTATION_OVERLAPPING_ADDRESSES 3

/// UTF-8 text the caller owns: `length` bytes at `bytes` (NULL when `length` is 0).
typedef struct {
    const char *bytes;
    size_t length;
} HkStr;

/// UTF-8 text the core wrote; release it with hk_text_free.
typedef struct {
    char *bytes;
    size_t length;
} HkText;

/// Texts the core wrote; release them with hk_text_list_free.
typedef struct {
    HkText *items;
    size_t count;
} HkTextList;

/// A dictation language as the host's locale reads its identifier (iOS:
/// Locale(identifier:).language.languageCode and .region); `code` or `region` with NULL bytes
/// is none. Functions taking a `const HkLanguage *` read NULL as no language (Swift's nil),
/// which most rules take as English.
typedef struct {
    HkStr identifier;
    HkStr code;
    HkStr region;
} HkLanguage;

/// The dictation style: 0 Polished, 1 Casual, 2 Notes, 3 Verbatim.
typedef struct {
    uint32_t style;
    /// Settings › Dictation › Format lists, Spoken punctuation, End with a period: nonzero on.
    uint8_t lists;
    uint8_t punctuation;
    uint8_t ends_with_period;
} HkFormatOptions;

/// Whether a lowercase word is in the dictation language's dictionary: nonzero for yes. Called
/// on the calling thread during the call, with `word` valid only while it runs.
typedef uint8_t (*HkIsWord)(void *context, HkStr word);

/// The style (TranscriptFormatter.format): fillers trimmed, Format lists and Spoken
/// punctuation, the style's sentences, and End with a period.
int32_t hk_dictation_format(HkStr text, const HkLanguage *language, const HkFormatOptions *options,
                            HkText *out);
/// The stages on their own, in the order the pipeline runs them: spoken symbols, spoken counts,
/// then (Clean up on) the host's English tagger or, for Spanish, German and Norwegian,
/// hk_dictation_cleanup_rules; later the style (with Format lists and Spoken punctuation inside
/// it), and French spacing last.
int32_t hk_dictation_spoken_symbols(HkStr text, const HkLanguage *language, HkText *out);
int32_t hk_dictation_spoken_counts(HkStr text, const HkLanguage *language, HkText *out);
int32_t hk_dictation_cleanup_rules(HkStr text, const HkLanguage *language, HkText *out);
int32_t hk_dictation_spoken_lists(HkStr text, const HkLanguage *language, HkText *out);
int32_t hk_dictation_spoken_punctuation(HkStr text, const HkLanguage *language, HkText *out);
int32_t hk_dictation_french_spacing(HkStr text, const HkLanguage *language, HkText *out);
/// French spacing by convention: 0 France, 1 Canada, 2 Switzerland.
int32_t hk_dictation_french_spacing_with(HkStr text, uint32_t convention, HkText *out);
/// The convention for `language` (0 France, 1 Canada, 2 Switzerland), or -1 when it isn't French.
int32_t hk_dictation_french_convention(const HkLanguage *language);

/// CleanupReconstruction.reconstruct: the words with one delete flag each (nonzero deletes;
/// fewer flags than words keep the rest), joined and repaired. Also the tagger's last step.
int32_t hk_dictation_reconstruct(const HkStr *words, size_t word_count, const uint8_t *deletes,
                                 size_t delete_count, HkText *out);
/// CleanupTagger.words(in:): the whitespace-separated words.
int32_t hk_dictation_words(HkStr text, HkTextList *out);
/// SpokenCleanupRules.bare: lowercased, without opening marks or trailing punctuation.
int32_t hk_dictation_bare(HkStr text, HkText *out);
/// SpokenCleanupRules.supports: `out` nonzero when Spanish, German or Norwegian rules apply.
int32_t hk_dictation_cleanup_supports(const HkLanguage *language, uint8_t *out);
/// TranscriptFormatter.fillerPattern(for:): a NUL-terminated pattern the core keeps, or NULL.
const char *hk_dictation_filler_pattern(const HkLanguage *language);

/// TranscriptFormatter.insertionText: `formatted` joined to the text around the cursor.
/// `before` and `after` are NULL for none; `is_word` NULL leaves the case alone.
int32_t hk_dictation_insertion_text(HkStr formatted, const HkStr *before, const HkStr *after,
                                    HkIsWord is_word, void *context, HkText *out);
/// TranscriptFormatter.continuingSentence.
int32_t hk_dictation_continuing_sentence(HkStr text, HkIsWord is_word, void *context, HkText *out);
/// TranscriptFormatter.startsSentence(after:); `before` NULL for none. `out` nonzero for yes.
int32_t hk_dictation_starts_sentence(const HkStr *before, uint8_t *out);
/// TranscriptFormatter.lowercasesContinuations(in:); `language` is needed.
int32_t hk_dictation_lowercases_continuations(const HkLanguage *language, uint8_t *out);
/// TranscriptFormatter.droppingFinalFullStop.
int32_t hk_dictation_dropping_final_full_stop(HkStr text, HkText *out);

/// Releases text the core wrote and empties it. NULL, or an empty one, is fine.
void hk_text_free(HkText *text);
void hk_text_list_free(HkTextList *list);

#ifdef __cplusplus
}
#endif

#endif
