#include <android/asset_manager_jni.h>
#include <jni.h>
#include <whisper.h>

#include <algorithm>
#include <memory>
#include <string>
#include <vector>

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ly_anoma_hex_speech_Whisper_decode(JNIEnv *env, jobject, jobject assets, jshortArray samples,
                                         jint count, jint threads) {
    if (count < 1 || count > env->GetArrayLength(samples)) return nullptr;
    auto manager = AAssetManager_fromJava(env, assets);
    if (!manager) return nullptr;
    std::unique_ptr<AAsset, decltype(&AAsset_close)> model(
        AAssetManager_open(manager, "whisper-tiny.bin", AASSET_MODE_BUFFER), AAsset_close);
    if (!model) return nullptr;
    auto data = const_cast<void *>(AAsset_getBuffer(model.get()));
    if (!data) return nullptr;

    auto options = whisper_context_default_params();
    options.use_gpu = false;
    std::unique_ptr<whisper_context, decltype(&whisper_free)> ctx(
        whisper_init_from_buffer_with_params(data, AAsset_getLength(model.get()), options), whisper_free);
    model.reset();
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
    params.language = "en";
    params.translate = false;
    params.no_context = true;
    params.no_timestamps = true;
    params.print_progress = params.print_realtime = params.print_timestamps = params.print_special = false;
    params.suppress_nst = true;
    if (whisper_full(ctx.get(), params, pcm.data(), count) != 0) return nullptr;
    std::string text;
    for (int i = 0; i < whisper_full_n_segments(ctx.get()); ++i)
        text += whisper_full_get_segment_text(ctx.get(), i);
    // JNI's modified UTF-8 is not standard UTF-8 (notably for supplementary code points).
    auto result = env->NewByteArray(static_cast<jsize>(text.size()));
    if (result) env->SetByteArrayRegion(result, 0, static_cast<jsize>(text.size()),
                                        reinterpret_cast<const jbyte *>(text.data()));
    return result;
}
