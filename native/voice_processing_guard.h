#ifndef HEX_VOICE_PROCESSING_GUARD_H
#define HEX_VOICE_PROCESSING_GUARD_H

#include <stdint.h>

/// Runs `body` inside @try/@catch, so an Objective-C exception AVAudioEngine
/// raises for a refused graph or a failed tap becomes an error Swift can fall
/// back from. Answers 0, or 1 with an owned message in `error` (free it with
/// free()).
typedef void (^hex_voice_processing_guard_body)(void);
int32_t hex_voice_processing_guard(hex_voice_processing_guard_body body, char **error);

#endif
