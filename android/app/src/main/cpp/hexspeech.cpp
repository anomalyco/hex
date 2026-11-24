#include <android/asset_manager_jni.h>
#include <dlfcn.h>
#include <jni.h>
#ifndef NDEBUG
#include <android/log.h>
#endif
#include <ggml-cpu.h>
#include <whisper.h>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cmath>
#include <cstring>
#include <iterator>
#include <memory>
#include <mutex>
#include <string>
#include <unordered_map>
#include <vector>

// The model stays loaded between dictations (SpeechModel.kt owns it and frees it when idle), so a
// dictation pays only for its own audio. Load, decode, threadPool and free are called under one lock.

// A model's thread pool (Whisper.threadPool): ggml's worker threads made once and reused for every graph, where by
// default ggml starts and joins a new pool for each one, every encoder pass and every decoded token. Only Labs ›
// Diagnostics' speed test asks for one today. A pool lives with its model and is freed with it.
struct Pool { ggml_threadpool *threads; int count; };
static std::mutex pools_lock;
static std::unordered_map<whisper_context *, Pool> pools;

static void drop_pool(whisper_context *ctx) {
    std::lock_guard<std::mutex> hold(pools_lock);
    auto found = pools.find(ctx);
    if (found == pools.end()) return;
    ggml_threadpool_free(found->second.threads);
    pools.erase(found);
}
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
    drop_pool(reinterpret_cast<whisper_context *>(ctx));
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

// Keeps a pool of [threads] worker threads with the model for its decodes, or none (0), ggml's default. A pool of
// another size is replaced. ggml's default pool parameters, as its per-graph pools use: only reuse differs.
extern "C" JNIEXPORT jboolean JNICALL
Java_ly_anoma_hex_speech_Whisper_threadPool(JNIEnv *, jobject, jlong handle, jint threads) {
    auto ctx = reinterpret_cast<whisper_context *>(handle);
    if (!ctx) return false;
    {
        std::lock_guard<std::mutex> hold(pools_lock);
        auto found = pools.find(ctx);
        if (found != pools.end() && found->second.count == threads) return true;
    }
    drop_pool(ctx);
    if (threads <= 0) return true;
    auto params = ggml_threadpool_params_default(threads);
    auto pool = ggml_threadpool_new(&params);
    if (!pool) return false;
    std::lock_guard<std::mutex> hold(pools_lock);
    pools[ctx] = {pool, threads};
    return true;
}

// The last decode's timings, for Labs › Diagnostics: microseconds spent sampling, in the encoder, decoding one token at a
// time, decoding in batches and on the prompt, then the encoder window (0: the full 30 s) and the thread count. Written
// by decode and read by timings, both under SpeechModel's lock.
static jlong last[7];

extern "C" JNIEXPORT jlongArray JNICALL
Java_ly_anoma_hex_speech_Whisper_timings(JNIEnv *env, jobject) {
    auto result = env->NewLongArray(7);
    if (result) env->SetLongArrayRegion(result, 0, 7, last);
    return result;
}

// The name of the file this code was loaded from (libhexspeech.so, or another build of it), for Labs › Diagnostics.
extern "C" JNIEXPORT jstring JNICALL
Java_ly_anoma_hex_speech_Whisper_library(JNIEnv *env, jobject) {
    Dl_info info{};
    if (!dladdr(reinterpret_cast<void *>(&Java_ly_anoma_hex_speech_Whisper_library), &info) || !info.dli_fname) return nullptr;
    const char *name = std::strrchr(info.dli_fname, '/');
    return env->NewStringUTF(name ? name + 1 : info.dli_fname);
}

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
    // Dictation asks for at most 4; Labs › Diagnostics also tries 6.
    params.n_threads = std::clamp<int>(threads, 1, 8);
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
    // Longer, the speech plus 12.8 s (640 frames), rounded up to 64, until that reaches Whisper's full
    // 1500 (about 16.7 s and more). On 53 long clips in six languages (8.5-30 s) that margin made no clip
    // worse than the full window; at 576 and less clips start to lose words, so don't lower it without
    // rerunning the long-clip check (benchmark/README.md). The bundled tiny model keeps the full window;
    // shortened, it repeats itself.
    if (small) {
        const int speech = static_cast<int>(std::ceil(count * 50.0 / 16000));
        const int frames = count <= 8 * 16000 ? std::min(512, (speech + 192 + 63) / 64 * 64) : (speech + 640 + 63) / 64 * 64;
        if (frames < 1500) params.audio_ctx = frames;
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
    // Audio longer than Whisper's 30 s window is decoded with timestamps, as Whisper decodes long audio: each window
    // then ends at the last sentence it finished, and the next starts there. Without them whisper.cpp moves on a
    // whole 30 s at a time, cutting mid-sentence: the words across the cut were lost, and a window full of speech to
    // its edge left out whole sentences (5 of 14 in a 36 s Spanish dictation). On 32 clips of 36-58 s in the ten
    // dictation languages that lost 27 of 457 sentences; with timestamps, one: a sentence said twice in a row,
    // which every setting drops. The time is the same. Up to 30 s nothing changes.
    params.no_timestamps = count <= 30 * 16000;
    params.print_progress = params.print_realtime = params.print_timestamps = params.print_special = false;
    params.suppress_nst = true;
    // A fresh decoding state for every dictation (3 ms), the weights staying loaded: a state kept from an
    // earlier, longer dictation leaves encoder output past a shorter window's end, which its cross-attention
    // still reads, so a short dictation after a long one could come out differently (even in another language).
    std::unique_ptr<whisper_state, decltype(&whisper_free_state)> state(whisper_init_state(ctx), whisper_free_state);
    if (!state) return nullptr;
    ggml_threadpool *pool = nullptr;
    {
        std::lock_guard<std::mutex> hold(pools_lock);
        auto found = pools.find(ctx);
        if (found != pools.end()) { pool = found->second.threads; params.n_threads = std::min(params.n_threads, found->second.count); }
    }
    // The pool outlives the state, which never frees it. A decode stopped by abort ends between graphs, so the pool is
    // idle and the next graph resets its abort flag.
    if (pool) hex_whisper_state_set_threadpool(state.get(), pool);
#ifndef NDEBUG
    const auto started = std::chrono::steady_clock::now();
#endif
    const int status = whisper_full_with_state(ctx, state.get(), params, pcm.data(), count);
    // Paused workers wait on a condition variable until the next graph instead of polling.
    if (pool) ggml_threadpool_pause(pool);
    whisper_timings spent{};
    hex_whisper_state_timings(state.get(), &spent);
    const jlong now[7] = {static_cast<jlong>(spent.sample_ms * 1000), static_cast<jlong>(spent.encode_ms * 1000),
                          static_cast<jlong>(spent.decode_ms * 1000), static_cast<jlong>(spent.batchd_ms * 1000),
                          static_cast<jlong>(spent.prompt_ms * 1000), params.audio_ctx, params.n_threads};
    std::copy(std::begin(now), std::end(now), std::begin(last));
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
