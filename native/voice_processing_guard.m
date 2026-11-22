#import "voice_processing_guard.h"

#import <Foundation/Foundation.h>

#include <stdlib.h>

int32_t hex_voice_processing_guard(hex_voice_processing_guard_body body, char **error) {
    @try {
        body();
        return 0;
    } @catch (NSException *exception) {
        if (error != NULL) {
            NSString *reason = exception.reason ?: exception.name ?: @"Objective-C exception";
            *error = strdup(reason.UTF8String);
        }
        return 1;
    }
}
