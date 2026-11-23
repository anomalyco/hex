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
/// -ln P(word) in nats; NaN past the last word.
double hk_model_unigram_cost(const HkModel *model, size_t word);
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

// MARK: - Autocorrect

// One keyboard's suggestions and replace-on-space (iOS's AutocorrectEngine): the language's
// engine and, with bilingual autocorrect, its partner's. A language is its BCP 47 tag ("en",
// "fr-CA", "sr-Latn"), whose manifest is compiled into the core (crates/keyboard/languages);
// an unknown tag turns everything off but your text replacements. A handle is used from one
// thread at a time. Unless a comment says otherwise, a function returns 0, or nonzero on a
// bad argument (1) or a failure (2), and leaves its out arguments NULL or empty then; strings
// out are released with hk_string_free, lists with hk_string_list_free.

typedef struct HkAutocorrect HkAutocorrect;
/// Strings a checker callback answers with.
typedef struct HkStrings HkStrings;
/// Adds `text` to the answer a guesses or completions callback is filling.
void hk_strings_push(HkStrings *list, const char *text);

/// The host's spelling dictionary (iOS: SystemTextChecker over UITextChecker). Each function
/// gets `context` and is called on the handle's thread, synchronously, during the call that
/// needs it, in the order the Swift engine asked; it must not call back into the handle.
/// `release` (NULL: nothing) is called once the core no longer needs the checker.
typedef struct {
    void *context;
    uint8_t (*is_word_valid)(void *context, const char *word);
    void (*guesses)(void *context, const char *word, HkStrings *out);
    void (*completions)(void *context, const char *prefix, HkStrings *out);
    void (*use_language)(void *context, const char *code);
    void (*release)(void *context);
} HkTextChecker;

/// The never-suggest list (the text of never-suggest.tsv), read once per language; NULL on a
/// bad argument.
typedef struct HkNeverSuggest HkNeverSuggest;
HkNeverSuggest *hk_never_suggest_new(const char *tsv);
void hk_never_suggest_free(HkNeverSuggest *list);
/// The kind of `word` (a spelling or two-word expression, any case) in `language`'s rows
/// ("en", "fr"; others read the shared * rows): 0 none, 1 a slur, 2 a swear word, -1 on a bad
/// argument.
int32_t hk_never_suggest_kind(const HkNeverSuggest *list, const char *language, const char *word);
/// Whether `candidate` may not be offered (it, a word of it, or an expression with `previous`,
/// NULL for none, is a slur, or a swear word without `allow_swearing`): 1 or 0, -1 on a bad
/// argument.
int32_t hk_never_suggest_blocks(const HkNeverSuggest *list, const char *language,
                                const char *candidate, uint8_t allow_swearing,
                                const char *previous);
typedef struct {
    char **items;
    size_t count;
} HkStringList;
void hk_string_list_free(HkStringList *list);
/// `language`'s spellings and expressions to block, sorted.
int32_t hk_never_suggest_blocked(const HkNeverSuggest *list, const char *language,
                                 uint8_t allow_swearing, HkStringList *out);

/// German's nouns' share of capitalised uses (de.capitals), read in place: mapped from a file
/// (NULL when it can't be or isn't a list), or a copy of a list's bytes.
typedef struct HkGermanCapitals HkGermanCapitals;
HkGermanCapitals *hk_german_capitals_open(const char *path);
HkGermanCapitals *hk_german_capitals_new(const uint8_t *bytes, size_t count);
void hk_german_capitals_free(HkGermanCapitals *capitals);
/// `word`'s share (50 to 100), 0 when the list doesn't know it, -1 on a bad argument.
int32_t hk_german_capitals_percent(const HkGermanCapitals *capitals, const char *word);
/// Whether `word`, all lowercase, takes a capital after `previous` (NULL: none): 1 yes, 0 no, 2
/// when the list doesn't know it, -1 on a bad argument.
int32_t hk_german_capitals_capitalises(const HkGermanCapitals *capitals, const char *word,
                                       const char *previous);

/// A new autocorrect in English. `never_suggest` and `german` (the bundled capitals) may be
/// NULL; the core keeps its own references. The checker is released on free (or at once,
/// with NULL returned, on a bad argument).
HkAutocorrect *hk_autocorrect_new(const HkNeverSuggest *never_suggest,
                                  const HkGermanCapitals *german, HkTextChecker checker);
void hk_autocorrect_free(HkAutocorrect *ac);
/// Settings › Keyboard › Autocorrect, Suggest swear words, Auto-capitalization, and Learn each
/// language separately: 0 off, else on.
int32_t hk_autocorrect_set_settings(HkAutocorrect *ac, uint8_t enabled, uint8_t allows_swearing,
                                    uint8_t adds_capitals, uint8_t learns_per_language);
/// Follows the keyboard layout: `tag` the language, `available` the device's dictionaries
/// (UITextChecker.availableLanguages). Clears what you kept this session.
int32_t hk_autocorrect_set_language(HkAutocorrect *ac, const char *tag,
                                    const char *const *available, size_t count);
/// Bilingual autocorrect's second language (`tag` NULL: none) with its own dictionary,
/// released when the core doesn't keep it.
int32_t hk_autocorrect_set_partner(HkAutocorrect *ac, const char *tag,
                                   const char *const *available, size_t count,
                                   HkTextChecker checker);
/// The keys the letters are typed on: `layout` the layout's language tag (whose manifest says
/// which accents a slip may leave out) and the letter grid the host shows, a cell per letter
/// with x its column and y its row (rows shifted as the keys are).
int32_t hk_autocorrect_set_keys(HkAutocorrect *ac, const char *layout, const HkKeyCentre *cells,
                                size_t count);
/// Your UILexicon entries: typing `inputs[i]` gives `outputs[i]`.
int32_t hk_autocorrect_load_lexicon(HkAutocorrect *ac, const char *const *inputs,
                                    const char *const *outputs, size_t count);
#define HK_AUTOCORRECT_VOCABULARY 0
#define HK_AUTOCORRECT_LEARNED 1
#define HK_AUTOCORRECT_PARTNER_LEARNED 2
/// Hex's vocabulary, or the words of the list of kept words this language (or the partner)
/// reads (LearnedWordsStore.words: the host keeps them), whenever they change.
int32_t hk_autocorrect_set_words(HkAutocorrect *ac, uint8_t which, const char *const *words,
                                 size_t count);
/// What Learn how I type learned (TypingLearning): typos and their fixes, and the words you
/// turned down.
typedef struct {
    const char *const *typos;
    const char *const *fixes;
    size_t typo_count;
    const char *const *rejected;
    size_t rejected_count;
} HkLearning;
/// With Learn each language separately on, the partner takes `partner` (NULL: `mine`).
int32_t hk_autocorrect_apply_learning(HkAutocorrect *ac, const HkLearning *mine,
                                      const HkLearning *partner);
/// Keeps `word` as typed this session; the host counts the keep in its LearnedWordsStore.
int32_t hk_autocorrect_suppress_and_keep(HkAutocorrect *ac, const char *word);
#define HK_AUTOCORRECT_DECODER 0
#define HK_AUTOCORRECT_PARTNER_DECODER 1
#define HK_AUTOCORRECT_CONTEXT 2
/// Spatial autocorrect's model, the partner's, or the layout language's word list and pairs
/// (contextModel); NULL for none. The core keeps its own reference.
int32_t hk_autocorrect_set_model(HkAutocorrect *ac, uint8_t which, const HkModel *model);
/// The letter keys taps are read against (`baseline` 0), or the centres read without Adaptive
/// touch (1); NULL for none.
int32_t hk_autocorrect_set_decoder_keys(HkAutocorrect *ac, uint8_t baseline,
                                        const HkTapKeys *keys);
/// Where each letter of the word being typed was tapped: `count` (x, y) pairs interleaved, or
/// `taps` NULL when the keys don't know.
int32_t hk_autocorrect_set_taps(HkAutocorrect *ac, const double *taps, size_t count);
/// This engine's German capitals (NULL: none); until set, the bundled ones.
int32_t hk_autocorrect_set_german_capitals(HkAutocorrect *ac, const HkGermanCapitals *capitals);

/// One slot of the strip: `kind` 0 keeps what was typed, 1 the best replacement, 2 the best
/// replacement space puts in, 3 the runner-up, 4 a word that can follow.
typedef struct {
    char *display_text;
    char *replacement;
    uint8_t kind;
} HkSuggestion;
typedef struct {
    HkSuggestion *items;
    size_t count;
} HkSuggestions;
/// The strip for the text before the cursor (`before` NULL when the host doesn't know it).
/// `field` 0 text, 1 URL, 2 email; `autocorrection_off` nonzero for a field that turns it off;
/// `prediction_model` the typing model predictions read (NULL: none); `capitalizes_next`
/// nonzero when shift would turn itself on. Release `out` with hk_suggestions_free.
int32_t hk_autocorrect_suggestions(HkAutocorrect *ac, const char *before, uint8_t field,
                                   uint8_t autocorrection_off, const HkModel *prediction_model,
                                   uint8_t capitalizes_next, HkSuggestions *out);
void hk_suggestions_free(HkSuggestions *list);
/// What space puts in: what was typed and its replacement, both NULL to keep it.
int32_t hk_autocorrect_replacement_on_space(HkAutocorrect *ac, const char *before, uint8_t field,
                                            uint8_t autocorrection_off, char **original,
                                            char **replacement);
/// Your text replacement whose shortcut ends `before`: the shortcut as typed and what it gives,
/// both NULL for none.
int32_t hk_autocorrect_text_replacement(HkAutocorrect *ac, const char *before, char **typed,
                                        char **replacement);
/// The word before the one being finished revised now that `next` follows it (`before`: the
/// text up to the end of `word`); NULL to leave it.
int32_t hk_autocorrect_revision(HkAutocorrect *ac, const char *word, const char *before,
                                const char *next, uint8_t field, uint8_t autocorrection_off,
                                char **out);
/// One word in place of `first` (with its `count` taps), a space tapped as a letter at
/// (`space_x`, `space_y`) and `second`, the word being finished (taps from
/// hk_autocorrect_set_taps); `before` the text up to the end of `first`. NULL to keep both.
int32_t hk_autocorrect_merge(HkAutocorrect *ac, const char *first, const double *first_taps,
                             size_t count, double space_x, double space_y, const char *second,
                             const char *before, char **out);
/// The word of `word`'s group ("id" or "I'd") the words around it make clearly likelier;
/// `previous` and `next` NULL for none. NULL to keep it.
int32_t hk_autocorrect_context_choice(HkAutocorrect *ac, const char *word, const char *previous,
                                      const char *next, uint8_t starts_sentence, char **out);
/// A string answer, by `which`: 0 `word` without a capital it only has from where it was
/// learned (typed as `other`), 1 the fix `word` as autocorrect puts it in for `other` typed
/// (NULL when that is what was typed), 2 your vocabulary's word `word` spells, 3 the language's
/// tag, 4 the partner's, 5 the language of the fix space last decided on, 6 that of the last
/// revision. NULL for none.
int32_t hk_autocorrect_text(HkAutocorrect *ac, uint8_t which, const char *word, const char *other,
                            char **out);
/// A yes-or-no answer, by `which`: 0 `word` may be offered after `other` (NULL: none), 1
/// `word` is a word as typed (the dictionary's, yours or shorthand), 2 `word` is one of your
/// own spellings, 3 `word` is one of your words in capitals read for `other` typed, 4 the
/// device has a dictionary for the language, 5 the language has words to correct with (its
/// dictionary or Hex's model), 6 the dictionary knows `word`. 1 or 0, -1 on a bad argument.
int32_t hk_autocorrect_is(HkAutocorrect *ac, uint8_t which, const char *word, const char *other);
/// Which language the words of `before` are in, counting words of `common_from` letters or
/// more by the typing models: above zero the partner's, below this one's. Nonzero without a
/// partner.
int32_t hk_autocorrect_language_lean(HkAutocorrect *ac, const char *before, size_t common_from,
                                     int32_t *out);
/// Bumped whenever your own words, the language or Suggest swear words change.
uint64_t hk_autocorrect_own_words_revision(HkAutocorrect *ac);
/// A list, by `which`: 0 your words for Swipe typing, 1 the words never offered in the
/// language, 2 `items` (the strip's words) without capitals that `word` didn't type, 3 a
/// swipe's readings `items` with your words in capitals never first unless `flag` (caps lock),
/// read with `model`.
int32_t hk_autocorrect_list(HkAutocorrect *ac, uint8_t which, const char *const *items,
                            size_t count, const char *word, uint8_t flag, const HkModel *model,
                            HkStringList *out);

/// A string from text alone, by `which`: 0 the word right before the caret in `a` (NULL for
/// none: none inside URLs, handles, paths or numbers), 1 the word before `b` in `a`,
/// lowercased, when only spaces separate them, 2 `b` cased as `a` was typed, 3 your vocabulary
/// word `b`'s spelling for `a` typed (NULL when that is what was typed). NULL for none.
int32_t hk_text(uint8_t which, const char *a, const char *b, char **out);
/// A yes-or-no from text alone, by `which`: 0 a sentence starts after `a` (NULL: an empty
/// field) in `language`, 1 `b`, at the end of `a`, starts a sentence, 2 `a` has a vowel, 3 `a`
/// is an abbreviation a period doesn't correct, 4 the core has `language`'s manifest. 1 or 0,
/// -1 on a bad argument.
int32_t hk_text_is(uint8_t which, const char *a, const char *b, const char *language);
/// The keyboard cost of a one-edit slip from `source` to `target` in `language` on the grid
/// (as hk_autocorrect_set_keys's): writes it, NaN when they're further apart.
int32_t hk_edit_cost(const char *source, const char *target, const char *language,
                     const HkKeyCentre *cells, size_t count, double *out);
/// Whether letters `a` and `b`'s keys neighbour on the grid: 1 or 0, -1 on a bad argument.
int32_t hk_adjacent_keys(const char *a, const char *b, const HkKeyCentre *cells, size_t count);
/// `language`'s words, by `which`: 0 sentence starters, 1 clause starters, 2 its commonest
/// words, 3 the English contractions Hex corrects to or offers (any language).
int32_t hk_language_words(const char *language, uint8_t which, HkStringList *out);

#ifdef __cplusplus
}
#endif

#endif
