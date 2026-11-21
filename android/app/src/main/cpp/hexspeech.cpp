#include <android/asset_manager_jni.h>
#include <jni.h>
#ifndef NDEBUG
#include <android/log.h>
#endif
#include <whisper.h>

#include <algorithm>
#include <memory>
#include <string>
#include <vector>

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ly_anoma_hex_speech_Whisper_decode(JNIEnv *env, jobject, jobject assets, jstring file,
                                         jshortArray samples, jint count, jint threads, jstring language,
                                         jboolean translate, jstring prompt) {
    if (count < 1 || count > env->GetArrayLength(samples)) return nullptr;
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
    std::unique_ptr<whisper_context, decltype(&whisper_free)> ctx(nullptr, whisper_free);
    if (file) {
        const char *path = env->GetStringUTFChars(file, nullptr);
        if (!path) return nullptr;
        ctx.reset(whisper_init_from_file_with_params(path, options));
        env->ReleaseStringUTFChars(file, path);
    } else {
        auto manager = AAssetManager_fromJava(env, assets);
        if (!manager) return nullptr;
        std::unique_ptr<AAsset, decltype(&AAsset_close)> model(
            AAssetManager_open(manager, "whisper-tiny.bin", AASSET_MODE_BUFFER), AAsset_close);
        if (!model) return nullptr;
        auto data = const_cast<void *>(AAsset_getBuffer(model.get()));
        if (!data) return nullptr;
        ctx.reset(whisper_init_from_buffer_with_params(data, AAsset_getLength(model.get()), options));
    }
    if (!ctx) return nullptr;

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
    // The measured short-dictation window; longer clips retain the full encoder context.
    if (file && count <= 8 * 16000) params.audio_ctx = 512;
    params.no_context = true;
    params.no_timestamps = true;
    params.print_progress = params.print_realtime = params.print_timestamps = params.print_special = false;
    params.suppress_nst = true;
    if (whisper_full(ctx.get(), params, pcm.data(), count) != 0) return nullptr;
#ifndef NDEBUG
    // Diagnostics identify the engine and task only; no audio or recognized text is logged.
    __android_log_print(ANDROID_LOG_DEBUG, "HexSpeech", "%s %s complete",
                        file ? "small" : "tiny", translate ? "translate" : "transcribe");
#endif
    std::string text;
    for (int i = 0; i < whisper_full_n_segments(ctx.get()); ++i)
        text += whisper_full_get_segment_text(ctx.get(), i);
    // JNI's modified UTF-8 is not standard UTF-8 (notably for supplementary code points).
    auto result = env->NewByteArray(static_cast<jsize>(text.size()));
    if (result) env->SetByteArrayRegion(result, 0, static_cast<jsize>(text.size()),
                                        reinterpret_cast<const jbyte *>(text.data()));
    return result;
}
