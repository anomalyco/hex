// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Kit Langton
//
// HexMicrophoneDriver.c
// Minimal CoreAudio AudioServerPlugIn virtual microphone driver for Hex.
//
// Exposes two devices inside coreaudiod:
//   1. "Hex Microphone" (UID: "anomaly.hex.microphone")
//      Visible 48 kHz mono float32 virtual input device for call apps.
//   2. "Hex Microphone (Internal Sink)" (UID: "anomaly.hex.microphone.sink")
//      Hidden 48 kHz mono float32 output sink that Hex writes processed capture
//      audio into.

#include <CoreAudio/AudioServerPlugIn.h>
#include <CoreFoundation/CoreFoundation.h>
#include <mach/mach_time.h>
#include <os/lock.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <string.h>

#define HEX_SAMPLE_RATE 48000.0
#define HEX_CHANNELS 1u
#define HEX_BITS_PER_CHANNEL 32u
#define HEX_BYTES_PER_FRAME 4u
#define HEX_ZERO_TIMESTAMP_PERIOD 4800u
#define HEX_SAFETY_OFFSET 64u

// Power-of-two capacity in mono float32 frames (16384 frames = ~341 ms at 48 kHz).
#define HEX_RING_CAPACITY 16384u
#define HEX_RING_MASK (HEX_RING_CAPACITY - 1u)
// Maximum queued frames before ReadInput skips ahead to keep latency < 20 ms
// (768 frames = 16 ms at 48 kHz).
#define HEX_MAX_RING_LATENCY_FRAMES 768u

enum {
    kObjectID_PlugIn = kAudioObjectPlugInObject, // 1
    kObjectID_MicDevice = 2,
    kObjectID_MicStream = 3,
    kObjectID_SinkDevice = 4,
    kObjectID_SinkStream = 5,
};

typedef struct {
    _Atomic uint64_t ioRunningCount;
    uint64_t anchorHostTime;
    uint64_t timeStampSeed;
    bool streamActive;
} HexDeviceState;

static os_unfair_lock gStateLock = OS_UNFAIR_LOCK_INIT;
static AudioServerPlugInHostRef gPlugInHost = NULL;
static uint32_t gPlugInRefCount = 0;
static Float64 gHostTicksPerFrame = 0.0;

static HexDeviceState gMicState = {
    .ioRunningCount = 0,
    .anchorHostTime = 0,
    .timeStampSeed = 1,
    .streamActive = true,
};

static HexDeviceState gSinkState = {
    .ioRunningCount = 0,
    .anchorHostTime = 0,
    .timeStampSeed = 1,
    .streamActive = true,
};

static float gRingBuffer[HEX_RING_CAPACITY];
static _Atomic uint64_t gRingWritePos = 0;
static _Atomic uint64_t gRingReadPos = 0;

static void HexRingReset(void) {
    atomic_store_explicit(&gRingWritePos, 0, memory_order_release);
    atomic_store_explicit(&gRingReadPos, 0, memory_order_release);
    memset(gRingBuffer, 0, sizeof(gRingBuffer));
}

static void HexRingWrite(const float *src, uint32_t frameCount) {
    uint64_t writePos = atomic_load_explicit(&gRingWritePos, memory_order_relaxed);
    uint64_t readPos = atomic_load_explicit(&gRingReadPos, memory_order_acquire);

    uint64_t available = writePos - readPos;
    if (available + frameCount > HEX_RING_CAPACITY) {
        // Advance readPos so the newest samples always fit without blocking.
        uint64_t newReadPos = writePos + frameCount - HEX_RING_CAPACITY;
        atomic_store_explicit(&gRingReadPos, newReadPos, memory_order_release);
    }

    for (uint32_t i = 0; i < frameCount; ++i) {
        gRingBuffer[(writePos + i) & HEX_RING_MASK] = src[i];
    }

    atomic_store_explicit(&gRingWritePos, writePos + frameCount, memory_order_release);
}

static void HexRingRead(float *dst, uint32_t frameCount) {
    if (atomic_load_explicit(&gSinkState.ioRunningCount, memory_order_acquire) == 0) {
        memset(dst, 0, (size_t)frameCount * sizeof(float));
        return;
    }

    uint64_t writePos = atomic_load_explicit(&gRingWritePos, memory_order_acquire);
    uint64_t readPos = atomic_load_explicit(&gRingReadPos, memory_order_relaxed);

    if (writePos < readPos) {
        readPos = writePos;
    }

    uint64_t available = writePos - readPos;
    if (available > HEX_MAX_RING_LATENCY_FRAMES + frameCount) {
        // Bound accumulated drift/queue latency to keep end-to-end delay < 20 ms.
        readPos = writePos - (HEX_MAX_RING_LATENCY_FRAMES / 2u + frameCount);
        available = writePos - readPos;
    }

    uint32_t toCopy = (available < (uint64_t)frameCount) ? (uint32_t)available : frameCount;
    for (uint32_t i = 0; i < toCopy; ++i) {
        dst[i] = gRingBuffer[(readPos + i) & HEX_RING_MASK];
    }
    if (toCopy < frameCount) {
        memset(dst + toCopy, 0, (size_t)(frameCount - toCopy) * sizeof(float));
    }

    atomic_store_explicit(&gRingReadPos, readPos + toCopy, memory_order_release);
}

static AudioStreamBasicDescription HexMakeStreamFormat(void) {
    AudioStreamBasicDescription desc = {
        .mSampleRate = HEX_SAMPLE_RATE,
        .mFormatID = kAudioFormatLinearPCM,
        .mFormatFlags = kAudioFormatFlagsNativeFloatPacked,
        .mBytesPerPacket = HEX_BYTES_PER_FRAME,
        .mFramesPerPacket = 1,
        .mBytesPerFrame = HEX_BYTES_PER_FRAME,
        .mChannelsPerFrame = HEX_CHANNELS,
        .mBitsPerChannel = HEX_BITS_PER_CHANNEL,
        .mReserved = 0,
    };
    return desc;
}

static AudioStreamRangedDescription HexMakeRangedStreamFormat(void) {
    AudioStreamRangedDescription desc = {
        .mFormat = HexMakeStreamFormat(),
        .mSampleRateRange = {
            .mMinimum = HEX_SAMPLE_RATE,
            .mMaximum = HEX_SAMPLE_RATE,
        },
    };
    return desc;
}

static bool HexIsDeviceObject(AudioObjectID inObjectID) {
    return inObjectID == kObjectID_MicDevice || inObjectID == kObjectID_SinkDevice;
}

static bool HexIsStreamObject(AudioObjectID inObjectID) {
    return inObjectID == kObjectID_MicStream || inObjectID == kObjectID_SinkStream;
}

static HexDeviceState *HexGetDeviceState(AudioObjectID inObjectID) {
    if (inObjectID == kObjectID_MicDevice || inObjectID == kObjectID_MicStream) {
        return &gMicState;
    }
    if (inObjectID == kObjectID_SinkDevice || inObjectID == kObjectID_SinkStream) {
        return &gSinkState;
    }
    return NULL;
}

// ---------------------------------------------------------------------------
// IUnknown
// ---------------------------------------------------------------------------

static HRESULT HexQueryInterface(void *inDriver, REFIID inUUID, LPVOID *outInterface) {
    if (outInterface == NULL) {
        return kAudioHardwareIllegalOperationError;
    }
    CFUUIDRef requested = CFUUIDCreateFromUUIDBytes(NULL, inUUID);
    if (requested == NULL) {
        *outInterface = NULL;
        return kAudioHardwareIllegalOperationError;
    }

    bool matches = CFEqual(requested, IUnknownUUID) ||
                   CFEqual(requested, kAudioServerPlugInDriverInterfaceUUID);
    CFRelease(requested);

    if (!matches) {
        *outInterface = NULL;
        return E_NOINTERFACE;
    }

    os_unfair_lock_lock(&gStateLock);
    ++gPlugInRefCount;
    os_unfair_lock_unlock(&gStateLock);

    *outInterface = inDriver;
    return kAudioHardwareNoError;
}

static ULONG HexAddRef(void *inDriver) {
    (void)inDriver;
    os_unfair_lock_lock(&gStateLock);
    ULONG count = ++gPlugInRefCount;
    os_unfair_lock_unlock(&gStateLock);
    return count;
}

static ULONG HexRelease(void *inDriver) {
    (void)inDriver;
    os_unfair_lock_lock(&gStateLock);
    ULONG count = (gPlugInRefCount > 0) ? --gPlugInRefCount : 0;
    os_unfair_lock_unlock(&gStateLock);
    return count;
}

// ---------------------------------------------------------------------------
// Lifecycle
// ---------------------------------------------------------------------------

static OSStatus HexInitialize(AudioServerPlugInDriverRef inDriver, AudioServerPlugInHostRef inHost) {
    (void)inDriver;
    os_unfair_lock_lock(&gStateLock);
    gPlugInHost = inHost;

    mach_timebase_info_data_t timebase = {0, 0};
    mach_timebase_info(&timebase);
    if (timebase.numer == 0) {
        timebase.numer = 1;
        timebase.denom = 1;
    }
    Float64 hostTicksPerSecond = (1000000000.0 * (Float64)timebase.denom) / (Float64)timebase.numer;
    gHostTicksPerFrame = hostTicksPerSecond / HEX_SAMPLE_RATE;

    HexRingReset();
    os_unfair_lock_unlock(&gStateLock);
    return kAudioHardwareNoError;
}

static OSStatus HexCreateDevice(
    AudioServerPlugInDriverRef inDriver,
    CFDictionaryRef inDescription,
    const AudioServerPlugInClientInfo *inClientInfo,
    AudioObjectID *outDeviceObjectID
) {
    (void)inDriver;
    (void)inDescription;
    (void)inClientInfo;
    (void)outDeviceObjectID;
    return kAudioHardwareUnsupportedOperationError;
}

static OSStatus HexDestroyDevice(AudioServerPlugInDriverRef inDriver, AudioObjectID inDeviceObjectID) {
    (void)inDriver;
    (void)inDeviceObjectID;
    return kAudioHardwareUnsupportedOperationError;
}

static OSStatus HexAddDeviceClient(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    const AudioServerPlugInClientInfo *inClientInfo
) {
    (void)inDriver;
    (void)inClientInfo;
    if (!HexIsDeviceObject(inDeviceObjectID)) {
        return kAudioHardwareBadObjectError;
    }
    return kAudioHardwareNoError;
}

static OSStatus HexRemoveDeviceClient(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    const AudioServerPlugInClientInfo *inClientInfo
) {
    (void)inDriver;
    (void)inClientInfo;
    if (!HexIsDeviceObject(inDeviceObjectID)) {
        return kAudioHardwareBadObjectError;
    }
    return kAudioHardwareNoError;
}

static OSStatus HexPerformDeviceConfigurationChange(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    UInt64 inChangeAction,
    void *inChangeInfo
) {
    (void)inDriver;
    (void)inDeviceObjectID;
    (void)inChangeAction;
    (void)inChangeInfo;
    return kAudioHardwareNoError;
}

static OSStatus HexAbortDeviceConfigurationChange(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    UInt64 inChangeAction,
    void *inChangeInfo
) {
    (void)inDriver;
    (void)inDeviceObjectID;
    (void)inChangeAction;
    (void)inChangeInfo;
    return kAudioHardwareNoError;
}

// ---------------------------------------------------------------------------
// PlugIn Properties
// ---------------------------------------------------------------------------

static Boolean HexPlugInHasProperty(
    const AudioObjectPropertyAddress *inAddress
) {
    switch (inAddress->mSelector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
        case kAudioObjectPropertyManufacturer:
        case kAudioObjectPropertyOwnedObjects:
        case kAudioPlugInPropertyDeviceList:
        case kAudioPlugInPropertyTranslateUIDToDevice:
        case kAudioPlugInPropertyResourceBundle:
        case kAudioPlugInPropertyBundleID:
            return true;
        default:
            return false;
    }
}

static OSStatus HexPlugInGetPropertyDataSize(
    const AudioObjectPropertyAddress *inAddress,
    UInt32 inQualifierDataSize,
    const void *inQualifierData,
    UInt32 *outDataSize
) {
    (void)inQualifierDataSize;
    (void)inQualifierData;
    switch (inAddress->mSelector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
            *outDataSize = sizeof(AudioClassID);
            return kAudioHardwareNoError;
        case kAudioObjectPropertyOwner:
        case kAudioPlugInPropertyTranslateUIDToDevice:
            *outDataSize = sizeof(AudioObjectID);
            return kAudioHardwareNoError;
        case kAudioObjectPropertyManufacturer:
        case kAudioPlugInPropertyResourceBundle:
        case kAudioPlugInPropertyBundleID:
            *outDataSize = sizeof(CFStringRef);
            return kAudioHardwareNoError;
        case kAudioObjectPropertyOwnedObjects:
        case kAudioPlugInPropertyDeviceList:
            *outDataSize = 2 * sizeof(AudioObjectID);
            return kAudioHardwareNoError;
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus HexPlugInGetPropertyData(
    const AudioObjectPropertyAddress *inAddress,
    UInt32 inQualifierDataSize,
    const void *inQualifierData,
    UInt32 inDataSize,
    UInt32 *outDataSize,
    void *outData
) {
    switch (inAddress->mSelector) {
        case kAudioObjectPropertyBaseClass:
            if (inDataSize < sizeof(AudioClassID)) return kAudioHardwareBadPropertySizeError;
            *((AudioClassID *)outData) = kAudioObjectClassID;
            *outDataSize = sizeof(AudioClassID);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyClass:
            if (inDataSize < sizeof(AudioClassID)) return kAudioHardwareBadPropertySizeError;
            *((AudioClassID *)outData) = kAudioPlugInClassID;
            *outDataSize = sizeof(AudioClassID);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyOwner:
            if (inDataSize < sizeof(AudioObjectID)) return kAudioHardwareBadPropertySizeError;
            *((AudioObjectID *)outData) = kAudioObjectUnknown;
            *outDataSize = sizeof(AudioObjectID);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyManufacturer:
            if (inDataSize < sizeof(CFStringRef)) return kAudioHardwareBadPropertySizeError;
            *((CFStringRef *)outData) = CFSTR("Hex");
            *outDataSize = sizeof(CFStringRef);
            return kAudioHardwareNoError;

        case kAudioPlugInPropertyBundleID:
            if (inDataSize < sizeof(CFStringRef)) return kAudioHardwareBadPropertySizeError;
            *((CFStringRef *)outData) = CFSTR("anomaly.hex.microphone.driver");
            *outDataSize = sizeof(CFStringRef);
            return kAudioHardwareNoError;

        case kAudioPlugInPropertyResourceBundle:
            if (inDataSize < sizeof(CFStringRef)) return kAudioHardwareBadPropertySizeError;
            *((CFStringRef *)outData) = CFSTR("");
            *outDataSize = sizeof(CFStringRef);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyOwnedObjects:
        case kAudioPlugInPropertyDeviceList: {
            AudioObjectID devices[2] = { kObjectID_MicDevice, kObjectID_SinkDevice };
            UInt32 count = inDataSize / sizeof(AudioObjectID);
            if (count > 2) count = 2;
            memcpy(outData, devices, count * sizeof(AudioObjectID));
            *outDataSize = count * sizeof(AudioObjectID);
            return kAudioHardwareNoError;
        }

        case kAudioPlugInPropertyTranslateUIDToDevice: {
            if (inDataSize < sizeof(AudioObjectID)) return kAudioHardwareBadPropertySizeError;
            if (inQualifierData == NULL || inQualifierDataSize < sizeof(CFStringRef)) {
                return kAudioHardwareBadPropertySizeError;
            }
            CFStringRef uid = *((const CFStringRef *)inQualifierData);
            AudioObjectID found = kAudioObjectUnknown;
            if (uid != NULL) {
                if (CFStringCompare(uid, CFSTR("anomaly.hex.microphone"), 0) == kCFCompareEqualTo) {
                    found = kObjectID_MicDevice;
                } else if (CFStringCompare(uid, CFSTR("anomaly.hex.microphone.sink"), 0) == kCFCompareEqualTo) {
                    found = kObjectID_SinkDevice;
                }
            }
            *((AudioObjectID *)outData) = found;
            *outDataSize = sizeof(AudioObjectID);
            return kAudioHardwareNoError;
        }

        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

// ---------------------------------------------------------------------------
// Device Properties
// ---------------------------------------------------------------------------

static bool HexDeviceScopeMatchesDirection(AudioObjectID inObjectID, AudioObjectPropertyScope inScope) {
    if (inObjectID == kObjectID_MicDevice) {
        return inScope == kAudioObjectPropertyScopeInput;
    }
    if (inObjectID == kObjectID_SinkDevice) {
        return inScope == kAudioObjectPropertyScopeOutput;
    }
    return false;
}

static Boolean HexDeviceHasProperty(
    AudioObjectID inObjectID,
    const AudioObjectPropertyAddress *inAddress
) {
    switch (inAddress->mSelector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
        case kAudioObjectPropertyName:
        case kAudioObjectPropertyManufacturer:
        case kAudioObjectPropertyOwnedObjects:
        case kAudioDevicePropertyDeviceUID:
        case kAudioDevicePropertyModelUID:
        case kAudioDevicePropertyTransportType:
        case kAudioDevicePropertyRelatedDevices:
        case kAudioDevicePropertyClockDomain:
        case kAudioDevicePropertyDeviceIsAlive:
        case kAudioDevicePropertyDeviceIsRunning:
        case kAudioDevicePropertyDeviceCanBeDefaultDevice:
        case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice:
        case kAudioDevicePropertyLatency:
        case kAudioDevicePropertyStreams:
        case kAudioObjectPropertyControlList:
        case kAudioDevicePropertySafetyOffset:
        case kAudioDevicePropertyNominalSampleRate:
        case kAudioDevicePropertyAvailableNominalSampleRates:
        case kAudioDevicePropertyIcon:
        case kAudioDevicePropertyIsHidden:
        case kAudioDevicePropertyZeroTimeStampPeriod:
        case kAudioDevicePropertyPreferredChannelsForStereo:
        case kAudioDevicePropertyPreferredChannelLayout:
            return true;
        default:
            (void)inObjectID;
            return false;
    }
}

static OSStatus HexDeviceGetPropertyDataSize(
    AudioObjectID inObjectID,
    const AudioObjectPropertyAddress *inAddress,
    UInt32 *outDataSize
) {
    switch (inAddress->mSelector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
            *outDataSize = sizeof(AudioClassID);
            return kAudioHardwareNoError;
        case kAudioObjectPropertyOwner:
        case kAudioDevicePropertyRelatedDevices:
            *outDataSize = sizeof(AudioObjectID);
            return kAudioHardwareNoError;
        case kAudioObjectPropertyName:
        case kAudioObjectPropertyManufacturer:
        case kAudioDevicePropertyDeviceUID:
        case kAudioDevicePropertyModelUID:
            *outDataSize = sizeof(CFStringRef);
            return kAudioHardwareNoError;
        case kAudioDevicePropertyIcon:
            *outDataSize = sizeof(CFURLRef);
            return kAudioHardwareNoError;
        case kAudioObjectPropertyOwnedObjects:
        case kAudioDevicePropertyStreams:
            if (inAddress->mScope == kAudioObjectPropertyScopeGlobal ||
                HexDeviceScopeMatchesDirection(inObjectID, inAddress->mScope)) {
                *outDataSize = sizeof(AudioObjectID);
            } else {
                *outDataSize = 0;
            }
            return kAudioHardwareNoError;
        case kAudioObjectPropertyControlList:
            *outDataSize = 0;
            return kAudioHardwareNoError;
        case kAudioDevicePropertyTransportType:
        case kAudioDevicePropertyClockDomain:
        case kAudioDevicePropertyDeviceIsAlive:
        case kAudioDevicePropertyDeviceIsRunning:
        case kAudioDevicePropertyDeviceCanBeDefaultDevice:
        case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice:
        case kAudioDevicePropertyLatency:
        case kAudioDevicePropertySafetyOffset:
        case kAudioDevicePropertyIsHidden:
        case kAudioDevicePropertyZeroTimeStampPeriod:
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;
        case kAudioDevicePropertyNominalSampleRate:
            *outDataSize = sizeof(Float64);
            return kAudioHardwareNoError;
        case kAudioDevicePropertyAvailableNominalSampleRates:
            *outDataSize = sizeof(AudioValueRange);
            return kAudioHardwareNoError;
        case kAudioDevicePropertyPreferredChannelsForStereo:
            *outDataSize = 2 * sizeof(UInt32);
            return kAudioHardwareNoError;
        case kAudioDevicePropertyPreferredChannelLayout:
            *outDataSize = offsetof(AudioChannelLayout, mChannelDescriptions) +
                           HEX_CHANNELS * sizeof(AudioChannelDescription);
            return kAudioHardwareNoError;
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus HexDeviceGetPropertyData(
    AudioObjectID inObjectID,
    const AudioObjectPropertyAddress *inAddress,
    UInt32 inDataSize,
    UInt32 *outDataSize,
    void *outData
) {
    bool isMic = (inObjectID == kObjectID_MicDevice);
    AudioObjectID streamID = isMic ? kObjectID_MicStream : kObjectID_SinkStream;
    HexDeviceState *dev = HexGetDeviceState(inObjectID);

    switch (inAddress->mSelector) {
        case kAudioObjectPropertyBaseClass:
            if (inDataSize < sizeof(AudioClassID)) return kAudioHardwareBadPropertySizeError;
            *((AudioClassID *)outData) = kAudioObjectClassID;
            *outDataSize = sizeof(AudioClassID);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyClass:
            if (inDataSize < sizeof(AudioClassID)) return kAudioHardwareBadPropertySizeError;
            *((AudioClassID *)outData) = kAudioDeviceClassID;
            *outDataSize = sizeof(AudioClassID);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyOwner:
            if (inDataSize < sizeof(AudioObjectID)) return kAudioHardwareBadPropertySizeError;
            *((AudioObjectID *)outData) = kObjectID_PlugIn;
            *outDataSize = sizeof(AudioObjectID);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyName:
            if (inDataSize < sizeof(CFStringRef)) return kAudioHardwareBadPropertySizeError;
            *((CFStringRef *)outData) = isMic
                ? CFSTR("Hex Microphone")
                : CFSTR("Hex Microphone (Internal Sink)");
            *outDataSize = sizeof(CFStringRef);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyManufacturer:
            if (inDataSize < sizeof(CFStringRef)) return kAudioHardwareBadPropertySizeError;
            *((CFStringRef *)outData) = CFSTR("Hex");
            *outDataSize = sizeof(CFStringRef);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyDeviceUID:
            if (inDataSize < sizeof(CFStringRef)) return kAudioHardwareBadPropertySizeError;
            *((CFStringRef *)outData) = isMic
                ? CFSTR("anomaly.hex.microphone")
                : CFSTR("anomaly.hex.microphone.sink");
            *outDataSize = sizeof(CFStringRef);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyModelUID:
            if (inDataSize < sizeof(CFStringRef)) return kAudioHardwareBadPropertySizeError;
            *((CFStringRef *)outData) = isMic
                ? CFSTR("anomaly.hex.microphone.model")
                : CFSTR("anomaly.hex.microphone.sink.model");
            *outDataSize = sizeof(CFStringRef);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyTransportType:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) = kAudioDeviceTransportTypeVirtual;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyRelatedDevices:
            if (inDataSize < sizeof(AudioObjectID)) return kAudioHardwareBadPropertySizeError;
            *((AudioObjectID *)outData) = inObjectID;
            *outDataSize = sizeof(AudioObjectID);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyClockDomain:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) = 0x4845584D; // 'HEXM'
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyDeviceIsAlive:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) = 1;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyDeviceIsRunning:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) =
                (atomic_load_explicit(&dev->ioRunningCount, memory_order_acquire) > 0) ? 1 : 0;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyDeviceCanBeDefaultDevice:
        case kAudioDevicePropertyDeviceCanBeDefaultSystemDevice:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) =
                (isMic && inAddress->mScope == kAudioObjectPropertyScopeInput) ? 1 : 0;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyLatency:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) = 0;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioDevicePropertySafetyOffset:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) = HEX_SAFETY_OFFSET;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyZeroTimeStampPeriod:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) = HEX_ZERO_TIMESTAMP_PERIOD;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyIsHidden:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            // Mic is visible to call apps; internal sink is hidden from output pickers.
            *((UInt32 *)outData) = isMic ? 0 : 1;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyNominalSampleRate:
            if (inDataSize < sizeof(Float64)) return kAudioHardwareBadPropertySizeError;
            *((Float64 *)outData) = HEX_SAMPLE_RATE;
            *outDataSize = sizeof(Float64);
            return kAudioHardwareNoError;

        case kAudioDevicePropertyAvailableNominalSampleRates: {
            if (inDataSize < sizeof(AudioValueRange)) return kAudioHardwareBadPropertySizeError;
            AudioValueRange range = { .mMinimum = HEX_SAMPLE_RATE, .mMaximum = HEX_SAMPLE_RATE };
            *((AudioValueRange *)outData) = range;
            *outDataSize = sizeof(AudioValueRange);
            return kAudioHardwareNoError;
        }

        case kAudioDevicePropertyIcon:
            if (inDataSize < sizeof(CFURLRef)) return kAudioHardwareBadPropertySizeError;
            *((CFURLRef *)outData) = NULL;
            *outDataSize = sizeof(CFURLRef);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyOwnedObjects:
        case kAudioDevicePropertyStreams:
            if (inAddress->mScope == kAudioObjectPropertyScopeGlobal ||
                HexDeviceScopeMatchesDirection(inObjectID, inAddress->mScope)) {
                if (inDataSize < sizeof(AudioObjectID)) {
                    *outDataSize = 0;
                    return kAudioHardwareNoError;
                }
                *((AudioObjectID *)outData) = streamID;
                *outDataSize = sizeof(AudioObjectID);
            } else {
                *outDataSize = 0;
            }
            return kAudioHardwareNoError;

        case kAudioObjectPropertyControlList:
            *outDataSize = 0;
            return kAudioHardwareNoError;

        case kAudioDevicePropertyPreferredChannelsForStereo: {
            if (inDataSize < 2 * sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            UInt32 *channels = (UInt32 *)outData;
            channels[0] = 1;
            channels[1] = 1;
            *outDataSize = 2 * sizeof(UInt32);
            return kAudioHardwareNoError;
        }

        case kAudioDevicePropertyPreferredChannelLayout: {
            UInt32 needed = offsetof(AudioChannelLayout, mChannelDescriptions) +
                            HEX_CHANNELS * sizeof(AudioChannelDescription);
            if (inDataSize < needed) return kAudioHardwareBadPropertySizeError;
            AudioChannelLayout *layout = (AudioChannelLayout *)outData;
            memset(layout, 0, needed);
            layout->mChannelLayoutTag = kAudioChannelLayoutTag_UseChannelDescriptions;
            layout->mNumberChannelDescriptions = HEX_CHANNELS;
            layout->mChannelDescriptions[0].mChannelLabel = kAudioChannelLabel_Mono;
            *outDataSize = needed;
            return kAudioHardwareNoError;
        }

        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

// ---------------------------------------------------------------------------
// Stream Properties
// ---------------------------------------------------------------------------

static Boolean HexStreamHasProperty(
    const AudioObjectPropertyAddress *inAddress
) {
    switch (inAddress->mSelector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
        case kAudioObjectPropertyOwner:
        case kAudioObjectPropertyOwnedObjects:
        case kAudioStreamPropertyIsActive:
        case kAudioStreamPropertyDirection:
        case kAudioStreamPropertyTerminalType:
        case kAudioStreamPropertyStartingChannel:
        case kAudioStreamPropertyLatency:
        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyPhysicalFormat:
        case kAudioStreamPropertyAvailableVirtualFormats:
        case kAudioStreamPropertyAvailablePhysicalFormats:
            return true;
        default:
            return false;
    }
}

static OSStatus HexStreamGetPropertyDataSize(
    const AudioObjectPropertyAddress *inAddress,
    UInt32 *outDataSize
) {
    switch (inAddress->mSelector) {
        case kAudioObjectPropertyBaseClass:
        case kAudioObjectPropertyClass:
            *outDataSize = sizeof(AudioClassID);
            return kAudioHardwareNoError;
        case kAudioObjectPropertyOwner:
            *outDataSize = sizeof(AudioObjectID);
            return kAudioHardwareNoError;
        case kAudioObjectPropertyOwnedObjects:
            *outDataSize = 0;
            return kAudioHardwareNoError;
        case kAudioStreamPropertyIsActive:
        case kAudioStreamPropertyDirection:
        case kAudioStreamPropertyTerminalType:
        case kAudioStreamPropertyStartingChannel:
        case kAudioStreamPropertyLatency:
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;
        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyPhysicalFormat:
            *outDataSize = sizeof(AudioStreamBasicDescription);
            return kAudioHardwareNoError;
        case kAudioStreamPropertyAvailableVirtualFormats:
        case kAudioStreamPropertyAvailablePhysicalFormats:
            *outDataSize = sizeof(AudioStreamRangedDescription);
            return kAudioHardwareNoError;
        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

static OSStatus HexStreamGetPropertyData(
    AudioObjectID inObjectID,
    const AudioObjectPropertyAddress *inAddress,
    UInt32 inDataSize,
    UInt32 *outDataSize,
    void *outData
) {
    bool isMicStream = (inObjectID == kObjectID_MicStream);
    HexDeviceState *dev = HexGetDeviceState(inObjectID);

    switch (inAddress->mSelector) {
        case kAudioObjectPropertyBaseClass:
            if (inDataSize < sizeof(AudioClassID)) return kAudioHardwareBadPropertySizeError;
            *((AudioClassID *)outData) = kAudioObjectClassID;
            *outDataSize = sizeof(AudioClassID);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyClass:
            if (inDataSize < sizeof(AudioClassID)) return kAudioHardwareBadPropertySizeError;
            *((AudioClassID *)outData) = kAudioStreamClassID;
            *outDataSize = sizeof(AudioClassID);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyOwner:
            if (inDataSize < sizeof(AudioObjectID)) return kAudioHardwareBadPropertySizeError;
            *((AudioObjectID *)outData) = isMicStream ? kObjectID_MicDevice : kObjectID_SinkDevice;
            *outDataSize = sizeof(AudioObjectID);
            return kAudioHardwareNoError;

        case kAudioObjectPropertyOwnedObjects:
            *outDataSize = 0;
            return kAudioHardwareNoError;

        case kAudioStreamPropertyIsActive:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            os_unfair_lock_lock(&gStateLock);
            *((UInt32 *)outData) = dev->streamActive ? 1 : 0;
            os_unfair_lock_unlock(&gStateLock);
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioStreamPropertyDirection:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            // 1 = input stream, 0 = output stream
            *((UInt32 *)outData) = isMicStream ? 1 : 0;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioStreamPropertyTerminalType:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) = isMicStream
                ? kAudioStreamTerminalTypeMicrophone
                : kAudioStreamTerminalTypeSpeaker;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioStreamPropertyStartingChannel:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) = 1;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioStreamPropertyLatency:
            if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
            *((UInt32 *)outData) = 0;
            *outDataSize = sizeof(UInt32);
            return kAudioHardwareNoError;

        case kAudioStreamPropertyVirtualFormat:
        case kAudioStreamPropertyPhysicalFormat:
            if (inDataSize < sizeof(AudioStreamBasicDescription)) return kAudioHardwareBadPropertySizeError;
            *((AudioStreamBasicDescription *)outData) = HexMakeStreamFormat();
            *outDataSize = sizeof(AudioStreamBasicDescription);
            return kAudioHardwareNoError;

        case kAudioStreamPropertyAvailableVirtualFormats:
        case kAudioStreamPropertyAvailablePhysicalFormats:
            if (inDataSize < sizeof(AudioStreamRangedDescription)) return kAudioHardwareBadPropertySizeError;
            *((AudioStreamRangedDescription *)outData) = HexMakeRangedStreamFormat();
            *outDataSize = sizeof(AudioStreamRangedDescription);
            return kAudioHardwareNoError;

        default:
            return kAudioHardwareUnknownPropertyError;
    }
}

// ---------------------------------------------------------------------------
// Property Dispatch
// ---------------------------------------------------------------------------

static Boolean HexHasProperty(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inObjectID,
    pid_t inClientProcessID,
    const AudioObjectPropertyAddress *inAddress
) {
    (void)inDriver;
    (void)inClientProcessID;
    if (inAddress == NULL) return false;

    if (inObjectID == kObjectID_PlugIn) {
        return HexPlugInHasProperty(inAddress);
    }
    if (HexIsDeviceObject(inObjectID)) {
        return HexDeviceHasProperty(inObjectID, inAddress);
    }
    if (HexIsStreamObject(inObjectID)) {
        return HexStreamHasProperty(inAddress);
    }
    return false;
}

static OSStatus HexIsPropertySettable(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inObjectID,
    pid_t inClientProcessID,
    const AudioObjectPropertyAddress *inAddress,
    Boolean *outIsSettable
) {
    (void)inDriver;
    (void)inClientProcessID;
    if (inAddress == NULL || outIsSettable == NULL) {
        return kAudioHardwareIllegalOperationError;
    }

    if (HexIsStreamObject(inObjectID) && inAddress->mSelector == kAudioStreamPropertyIsActive) {
        *outIsSettable = true;
        return kAudioHardwareNoError;
    }

    if (!HexHasProperty(inDriver, inObjectID, inClientProcessID, inAddress)) {
        return kAudioHardwareUnknownPropertyError;
    }

    *outIsSettable = false;
    return kAudioHardwareNoError;
}

static OSStatus HexGetPropertyDataSize(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inObjectID,
    pid_t inClientProcessID,
    const AudioObjectPropertyAddress *inAddress,
    UInt32 inQualifierDataSize,
    const void *inQualifierData,
    UInt32 *outDataSize
) {
    (void)inDriver;
    (void)inClientProcessID;
    if (inAddress == NULL || outDataSize == NULL) {
        return kAudioHardwareIllegalOperationError;
    }

    if (inObjectID == kObjectID_PlugIn) {
        return HexPlugInGetPropertyDataSize(inAddress, inQualifierDataSize, inQualifierData, outDataSize);
    }
    if (HexIsDeviceObject(inObjectID)) {
        return HexDeviceGetPropertyDataSize(inObjectID, inAddress, outDataSize);
    }
    if (HexIsStreamObject(inObjectID)) {
        return HexStreamGetPropertyDataSize(inAddress, outDataSize);
    }
    return kAudioHardwareBadObjectError;
}

static OSStatus HexGetPropertyData(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inObjectID,
    pid_t inClientProcessID,
    const AudioObjectPropertyAddress *inAddress,
    UInt32 inQualifierDataSize,
    const void *inQualifierData,
    UInt32 inDataSize,
    UInt32 *outDataSize,
    void *outData
) {
    (void)inDriver;
    (void)inClientProcessID;
    if (inAddress == NULL || outDataSize == NULL || outData == NULL) {
        return kAudioHardwareIllegalOperationError;
    }

    if (inObjectID == kObjectID_PlugIn) {
        return HexPlugInGetPropertyData(
            inAddress,
            inQualifierDataSize,
            inQualifierData,
            inDataSize,
            outDataSize,
            outData
        );
    }
    if (HexIsDeviceObject(inObjectID)) {
        return HexDeviceGetPropertyData(inObjectID, inAddress, inDataSize, outDataSize, outData);
    }
    if (HexIsStreamObject(inObjectID)) {
        return HexStreamGetPropertyData(inObjectID, inAddress, inDataSize, outDataSize, outData);
    }
    return kAudioHardwareBadObjectError;
}

static OSStatus HexSetPropertyData(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inObjectID,
    pid_t inClientProcessID,
    const AudioObjectPropertyAddress *inAddress,
    UInt32 inQualifierDataSize,
    const void *inQualifierData,
    UInt32 inDataSize,
    const void *inData
) {
    (void)inDriver;
    (void)inClientProcessID;
    (void)inQualifierDataSize;
    (void)inQualifierData;
    if (inAddress == NULL || inData == NULL) {
        return kAudioHardwareIllegalOperationError;
    }

    if (HexIsStreamObject(inObjectID) && inAddress->mSelector == kAudioStreamPropertyIsActive) {
        if (inDataSize < sizeof(UInt32)) return kAudioHardwareBadPropertySizeError;
        HexDeviceState *dev = HexGetDeviceState(inObjectID);
        os_unfair_lock_lock(&gStateLock);
        dev->streamActive = (*((const UInt32 *)inData) != 0);
        os_unfair_lock_unlock(&gStateLock);
        return kAudioHardwareNoError;
    }

    return kAudioHardwareUnsupportedOperationError;
}

// ---------------------------------------------------------------------------
// IO Operations
// ---------------------------------------------------------------------------

static OSStatus HexStartIO(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    UInt32 inClientID
) {
    (void)inDriver;
    (void)inClientID;
    HexDeviceState *dev = HexGetDeviceState(inDeviceObjectID);
    if (dev == NULL) {
        return kAudioHardwareBadObjectError;
    }

    os_unfair_lock_lock(&gStateLock);
    uint64_t prev = atomic_fetch_add_explicit(&dev->ioRunningCount, 1, memory_order_acq_rel);
    if (prev == 0) {
        dev->anchorHostTime = mach_absolute_time();
        dev->timeStampSeed += 1;
        if (inDeviceObjectID == kObjectID_SinkDevice) {
            HexRingReset();
        }
    }
    os_unfair_lock_unlock(&gStateLock);
    return kAudioHardwareNoError;
}

static OSStatus HexStopIO(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    UInt32 inClientID
) {
    (void)inDriver;
    (void)inClientID;
    HexDeviceState *dev = HexGetDeviceState(inDeviceObjectID);
    if (dev == NULL) {
        return kAudioHardwareBadObjectError;
    }

    os_unfair_lock_lock(&gStateLock);
    uint64_t current = atomic_load_explicit(&dev->ioRunningCount, memory_order_acquire);
    if (current > 0) {
        uint64_t prev = atomic_fetch_sub_explicit(&dev->ioRunningCount, 1, memory_order_acq_rel);
        if (prev == 1 && inDeviceObjectID == kObjectID_SinkDevice) {
            HexRingReset();
        }
    }
    os_unfair_lock_unlock(&gStateLock);
    return kAudioHardwareNoError;
}

static OSStatus HexGetZeroTimeStamp(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    UInt32 inClientID,
    Float64 *outSampleTime,
    UInt64 *outHostTime,
    UInt64 *outSeed
) {
    (void)inDriver;
    (void)inClientID;
    HexDeviceState *dev = HexGetDeviceState(inDeviceObjectID);
    if (dev == NULL || outSampleTime == NULL || outHostTime == NULL || outSeed == NULL) {
        return kAudioHardwareBadObjectError;
    }

    os_unfair_lock_lock(&gStateLock);
    UInt64 anchor = dev->anchorHostTime;
    UInt64 seed = dev->timeStampSeed;
    Float64 ticksPerFrame = gHostTicksPerFrame;
    os_unfair_lock_unlock(&gStateLock);

    UInt64 now = mach_absolute_time();
    Float64 ticksPerPeriod = ticksPerFrame * (Float64)HEX_ZERO_TIMESTAMP_PERIOD;
    UInt64 elapsedTicks = (now > anchor) ? (now - anchor) : 0;
    UInt64 periods = (ticksPerPeriod > 0.0) ? (UInt64)((Float64)elapsedTicks / ticksPerPeriod) : 0;

    *outSampleTime = (Float64)(periods * HEX_ZERO_TIMESTAMP_PERIOD);
    *outHostTime = anchor + (UInt64)((Float64)periods * ticksPerPeriod);
    *outSeed = seed;
    return kAudioHardwareNoError;
}

static OSStatus HexWillDoIOOperation(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    UInt32 inClientID,
    UInt32 inOperationID,
    Boolean *outWillDo,
    Boolean *outWillDoInPlace
) {
    (void)inDriver;
    (void)inClientID;
    if (outWillDo == NULL || outWillDoInPlace == NULL) {
        return kAudioHardwareIllegalOperationError;
    }

    if (inDeviceObjectID == kObjectID_MicDevice) {
        *outWillDo = (inOperationID == kAudioServerPlugInIOOperationReadInput);
        *outWillDoInPlace = true;
        return kAudioHardwareNoError;
    }
    if (inDeviceObjectID == kObjectID_SinkDevice) {
        *outWillDo = (inOperationID == kAudioServerPlugInIOOperationWriteMix);
        *outWillDoInPlace = true;
        return kAudioHardwareNoError;
    }

    return kAudioHardwareBadObjectError;
}

static OSStatus HexBeginIOOperation(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    UInt32 inClientID,
    UInt32 inOperationID,
    UInt32 inIOBufferFrameSize,
    const AudioServerPlugInIOCycleInfo *inIOCycleInfo
) {
    (void)inDriver;
    (void)inDeviceObjectID;
    (void)inClientID;
    (void)inOperationID;
    (void)inIOBufferFrameSize;
    (void)inIOCycleInfo;
    return kAudioHardwareNoError;
}

static OSStatus HexDoIOOperation(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    AudioObjectID inStreamObjectID,
    UInt32 inClientID,
    UInt32 inOperationID,
    UInt32 inIOBufferFrameSize,
    const AudioServerPlugInIOCycleInfo *inIOCycleInfo,
    void *ioMainBuffer,
    void *ioSecondaryBuffer
) {
    (void)inDriver;
    (void)inStreamObjectID;
    (void)inClientID;
    (void)inIOCycleInfo;
    (void)ioSecondaryBuffer;

    if (ioMainBuffer == NULL || inIOBufferFrameSize == 0) {
        return kAudioHardwareNoError;
    }

    if (inDeviceObjectID == kObjectID_SinkDevice &&
        inOperationID == kAudioServerPlugInIOOperationWriteMix) {
        HexRingWrite((const float *)ioMainBuffer, inIOBufferFrameSize);
        return kAudioHardwareNoError;
    }

    if (inDeviceObjectID == kObjectID_MicDevice &&
        inOperationID == kAudioServerPlugInIOOperationReadInput) {
        HexRingRead((float *)ioMainBuffer, inIOBufferFrameSize);
        return kAudioHardwareNoError;
    }

    return kAudioHardwareNoError;
}

static OSStatus HexEndIOOperation(
    AudioServerPlugInDriverRef inDriver,
    AudioObjectID inDeviceObjectID,
    UInt32 inClientID,
    UInt32 inOperationID,
    UInt32 inIOBufferFrameSize,
    const AudioServerPlugInIOCycleInfo *inIOCycleInfo
) {
    (void)inDriver;
    (void)inDeviceObjectID;
    (void)inClientID;
    (void)inOperationID;
    (void)inIOBufferFrameSize;
    (void)inIOCycleInfo;
    return kAudioHardwareNoError;
}

// ---------------------------------------------------------------------------
// Driver Interface Table & Factory Entry Point
// ---------------------------------------------------------------------------

static AudioServerPlugInDriverInterface gDriverInterface = {
    ._reserved = NULL,
    .QueryInterface = HexQueryInterface,
    .AddRef = HexAddRef,
    .Release = HexRelease,
    .Initialize = HexInitialize,
    .CreateDevice = HexCreateDevice,
    .DestroyDevice = HexDestroyDevice,
    .AddDeviceClient = HexAddDeviceClient,
    .RemoveDeviceClient = HexRemoveDeviceClient,
    .PerformDeviceConfigurationChange = HexPerformDeviceConfigurationChange,
    .AbortDeviceConfigurationChange = HexAbortDeviceConfigurationChange,
    .HasProperty = HexHasProperty,
    .IsPropertySettable = HexIsPropertySettable,
    .GetPropertyDataSize = HexGetPropertyDataSize,
    .GetPropertyData = HexGetPropertyData,
    .SetPropertyData = HexSetPropertyData,
    .StartIO = HexStartIO,
    .StopIO = HexStopIO,
    .GetZeroTimeStamp = HexGetZeroTimeStamp,
    .WillDoIOOperation = HexWillDoIOOperation,
    .BeginIOOperation = HexBeginIOOperation,
    .DoIOOperation = HexDoIOOperation,
    .EndIOOperation = HexEndIOOperation,
};

static AudioServerPlugInDriverInterface *gDriverInterfacePtr = &gDriverInterface;
static AudioServerPlugInDriverRef gDriverRef = &gDriverInterfacePtr;

__attribute__((visibility("default")))
void *HexMicrophoneDriverCreate(CFAllocatorRef inAllocator, CFUUIDRef inRequestedTypeUUID) {
    (void)inAllocator;
    if (inRequestedTypeUUID == NULL) {
        return NULL;
    }
    if (!CFEqual(inRequestedTypeUUID, kAudioServerPlugInTypeUUID)) {
        return NULL;
    }
    return gDriverRef;
}
