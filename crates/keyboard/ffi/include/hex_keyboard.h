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
#define HK_ABI_VERSION 1

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

#ifdef __cplusplus
}
#endif

#endif
