#include <android/asset_manager_jni.h>
#include <jni.h>
#ifndef NDEBUG
#include <android/log.h>
#endif
#include <whisper.h>

#include <algorithm>
#include <cmath>
#include <memory>
#include <string>
#include <vector>

// The model stays loaded between dictations (SpeechModel.kt owns it and frees it when idle), so a
// dictation pays only for its own audio. Load, decode and free are called under one lock.
extern "C" JNIEXPORT jlong JNICALL
Java_ly_anoma_hex_speech_Whisper_load(JNIEnv *env, jobject, jobject assets, jstring file) {
#ifdef NDEBUG
    // whisper.cpp also installs this callback for ggml. Set it before either engine starts.
    static const bool quiet = [] {
        whisper_log_set([](ggml_log_level, const char *, void *) {}, nullptr);
        return true;
    }();
    (void)quiet;
#endif
    auto options = whisper_context_default_params();
    options.use_gpu = false;
    whisper_context *ctx = nullptr;
    if (file) {
        const char *path = env->GetStringUTFChars(file, nullptr);
        if (!path) return 0;
        ctx = whisper_init_from_file_with_params(path, options);
        env->ReleaseStringUTFChars(file, path);
    } else {
        auto manager = AAssetManager_fromJava(env, assets);
        if (!manager) return 0;
        std::unique_ptr<AAsset, decltype(&AAsset_close)> model(
            AAssetManager_open(manager, "whisper-tiny.bin", AASSET_MODE_BUFFER), AAsset_close);
        if (!model) return 0;
        auto data = const_cast<void *>(AAsset_getBuffer(model.get()));
        if (!data) return 0;
        // The weights are copied into the context, so the asset can close afterwards.
        ctx = whisper_init_from_buffer_with_params(data, AAsset_getLength(model.get()), options);
    }
#ifndef NDEBUG
    __android_log_print(ANDROID_LOG_DEBUG, "HexSpeech", "%s model %s", file ? "small" : "tiny", ctx ? "loaded" : "failed");
#endif
    return reinterpret_cast<jlong>(ctx);
}

extern "C" JNIEXPORT void JNICALL
Java_ly_anoma_hex_speech_Whisper_free(JNIEnv *, jobject, jlong ctx) {
    whisper_free(reinterpret_cast<whisper_context *>(ctx));
#ifndef NDEBUG
    __android_log_print(ANDROID_LOG_DEBUG, "HexSpeech", "model freed");
#endif
}

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ly_anoma_hex_speech_Whisper_decode(JNIEnv *env, jobject, jlong handle, jboolean small, jshortArray samples,
                                         jint count, jint threads, jstring language, jboolean translate, jstring prompt) {
    auto ctx = reinterpret_cast<whisper_context *>(handle);
    if (!ctx || count < 1 || count > env->GetArrayLength(samples)) return nullptr;
    std::vector<float> pcm(count);
    jshort page[2048];
    for (int i = 0; i < count; i += 2048) {
        const int n = std::min(2048, count - i);
        env->GetShortArrayRegion(samples, i, n, page);
        if (env->ExceptionCheck()) return nullptr;
        for (int j = 0; j < n; ++j) pcm[i + j] = page[j] / 32768.0f;
    }
    auto params = whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
    params.n_threads = std::clamp<int>(threads, 1, 4);
    const char *code = env->GetStringUTFChars(language, nullptr);
    if (!code) return nullptr;
    std::string lang(code);
    env->ReleaseStringUTFChars(language, code);
    params.language = lang.c_str();
    params.translate = translate;
    // Custom vocabulary as Whisper's initial prompt biases its spelling of those names.
    std::string hint;
    if (prompt) {
        const char *chars = env->GetStringUTFChars(prompt, nullptr);
        if (!chars) return nullptr;
        hint = chars;
        env->ReleaseStringUTFChars(prompt, chars);
    }
    if (!hint.empty()) params.initial_prompt = hint.c_str();
    // The encoder's window, in 20 ms frames: up to 8 s of speech, the speech plus 3.84 s (192 frames),
    // at most the measured 512; the 40-clip benchmark kept the same word errors at 27% less time.
    // Longer clips keep the full window: every shorter one tried cost them accuracy. The bundled
    // tiny model keeps the full window too; shortened, it repeats itself.
    if (small && count <= 8 * 16000) {
        const int frames = static_cast<int>(std::ceil(count * 50.0 / 16000)) + 192;
        params.audio_ctx = std::min(512, (frames + 63) / 64 * 64);
    }
    params.no_context = true;
    params.no_timestamps = true;
    params.print_progress = params.print_realtime = params.print_timestamps = params.print_special = false;
    params.suppress_nst = true;
    // A fresh decoding state for every dictation (3 ms), the weights staying loaded: a state kept from an
    // earlier, longer dictation leaves encoder output past a shorter window's end, which its cross-attention
    // still reads, so a short dictation after a long one could come out differently (even in another language).
    std::unique_ptr<whisper_state, decltype(&whisper_free_state)> state(whisper_init_state(ctx), whisper_free_state);
    if (!state) return nullptr;
    if (whisper_full_with_state(ctx, state.get(), params, pcm.data(), count) != 0) return nullptr;
#ifndef NDEBUG
    // Diagnostics identify the engine and task only; no audio or recognized text is logged.
    __android_log_print(ANDROID_LOG_DEBUG, "HexSpeech", "%s %s complete, %d threads, window %d",
                        small ? "small" : "tiny", translate ? "translate" : "transcribe", params.n_threads, params.audio_ctx);
#endif
    std::string text;
    for (int i = 0; i < whisper_full_n_segments_from_state(state.get()); ++i)
        text += whisper_full_get_segment_text_from_state(state.get(), i);
    // JNI's modified UTF-8 is not standard UTF-8 (notably for supplementary code points).
    auto result = env->NewByteArray(static_cast<jsize>(text.size()));
    if (result) env->SetByteArrayRegion(result, 0, static_cast<jsize>(text.size()),
                                        reinterpret_cast<const jbyte *>(text.data()));
    return result;
}
