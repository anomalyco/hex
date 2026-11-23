#include <android/asset_manager_jni.h>
#include <jni.h>
#ifndef NDEBUG
#include <android/log.h>
#endif
#include <whisper.h>

#include <algorithm>
#include <atomic>
#include <chrono>
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

// The dictation the person discarded (✕, or Home's Cancel), by MicCapture's job number. whisper.cpp asks after each
// encoder pass and each decoded token, so its decode ends within one of those and the next dictation's load and
// decode no longer wait for the whole abandoned one. Set from any thread, outside the model's lock.
static std::atomic<jlong> abandoned{0};

extern "C" JNIEXPORT void JNICALL
Java_ly_anoma_hex_speech_Whisper_abort(JNIEnv *, jobject, jlong job) { abandoned.store(job); }

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ly_anoma_hex_speech_Whisper_decode(JNIEnv *env, jobject, jlong handle, jboolean small, jshortArray samples,
                                         jint count, jint threads, jstring language, jboolean translate, jstring prompt,
                                         jlong job) {
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
    // A language the model handles badly (Kazakh told English) repeats itself to Whisper's 220-token limit, then
    // retries with five sampled decoders at each of up to five more temperatures: 2–5× the time, for nonsense.
    // Speech measured at most 10 tokens a second (Kazakh in Kazakh; Swedish 6, English 3.6), so each 30 s window
    // may have 15 a second plus 16, and a decode that fails Whisper's checks is retried once, with one decoder,
    // at 0.2. whisper.cpp tries params.temperature, then each step up to 1.0, and decodes greedily at any
    // temperature <= 0: from -0.8 in steps of 1.0 that is greedy, then 0.2. A single retry any hotter (0.6, 1.0)
    // invented fluent English or mixed scripts instead. The 40 benchmark clips and 15 long ones never fall back,
    // and keep the same text.
    params.max_tokens = std::min(220, static_cast<int>(std::ceil(std::min(count, 30 * 16000) * 15.0 / 16000)) + 16);
    params.greedy.best_of = 1;
    params.temperature = -0.8f;
    params.temperature_inc = 1.0f;
    params.abort_callback = [](void *mine) { return abandoned.load() == *static_cast<const jlong *>(mine); };
    params.abort_callback_user_data = &job;
    params.no_context = true;
    params.no_timestamps = true;
    params.print_progress = params.print_realtime = params.print_timestamps = params.print_special = false;
    params.suppress_nst = true;
    // A fresh decoding state for every dictation (3 ms), the weights staying loaded: a state kept from an
    // earlier, longer dictation leaves encoder output past a shorter window's end, which its cross-attention
    // still reads, so a short dictation after a long one could come out differently (even in another language).
    std::unique_ptr<whisper_state, decltype(&whisper_free_state)> state(whisper_init_state(ctx), whisper_free_state);
    if (!state) return nullptr;
#ifndef NDEBUG
    const auto started = std::chrono::steady_clock::now();
#endif
    const int status = whisper_full_with_state(ctx, state.get(), params, pcm.data(), count);
#ifndef NDEBUG
    // Diagnostics identify the engine and task only; no audio or recognized text is logged.
    const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - started).count();
    __android_log_print(ANDROID_LOG_DEBUG, "HexSpeech", "%s %s %s in %lld ms, %d samples, %d threads, window %d, max %d tokens",
                        small ? "small" : "tiny", translate ? "translate" : "transcribe",
                        status == 0 ? "complete" : abandoned.load() == job ? "abandoned" : "failed", static_cast<long long>(ms),
                        count, params.n_threads, params.audio_ctx, params.max_tokens);
#endif
    if (status != 0) return nullptr;
    std::string text;
    for (int i = 0; i < whisper_full_n_segments_from_state(state.get()); ++i)
        text += whisper_full_get_segment_text_from_state(state.get(), i);
    // JNI's modified UTF-8 is not standard UTF-8 (notably for supplementary code points).
    auto result = env->NewByteArray(static_cast<jsize>(text.size()));
    if (result) env->SetByteArrayRegion(result, 0, static_cast<jsize>(text.size()),
                                        reinterpret_cast<const jbyte *>(text.data()));
    return result;
}
