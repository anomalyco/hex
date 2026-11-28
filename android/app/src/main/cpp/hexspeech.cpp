#include <android/asset_manager_jni.h>
#include <dlfcn.h>
#include <jni.h>
#ifndef NDEBUG
#include <android/log.h>
#endif
#include <ggml-cpu.h>
#include <whisper.h>

#include <sched.h>

#include <algorithm>
#include <atomic>
#include <cctype>
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
// default ggml starts and joins a new pool for each one, every encoder pass and every decoded token. Dictation keeps one
// with its model (SpeechModel.open); Labs › Diagnostics' speed test makes its own. A pool lives with its model and is
// freed with it.
struct Pool { ggml_threadpool *threads; int count; jlong mask; };
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

// A bit per core in [set].
static jlong core_bits(const cpu_set_t &set) {
    jlong bits = 0;
    for (int core = 0; core < 64; ++core) if (CPU_ISSET(core, &set)) bits |= static_cast<jlong>(1) << core;
    return bits;
}

// [fast]'s cores, or 0 when they are fewer than the pool has threads: sharing them is slower than letting the kernel
// place the threads, as before. Also 0 when the kernel won't pin this thread to exactly them (tried, read back and put
// back here), so a pool is either pinned to what Labs › Diagnostics reports or not pinned at all. The fast cores are
// asked for whichever cores this thread may run on now: HyperOS narrows the UI thread to cpu 4-6 while a touch is boosted
// (Xiaomi 13T Pro, MT6985, #3037) and threads started then keep that, so intersecting with it left 3 of 4 fast cores
// and the pool unpinned. sched_setaffinity widens it again where the cpuset allows; the read-back checks that.
static jlong pool_mask(jlong fast, int threads) {
    if (fast == 0) return 0;
    cpu_set_t allowed;
    if (sched_getaffinity(0, sizeof(allowed), &allowed) != 0) return 0;
    jlong usable = 0;
    int cores = 0;
    cpu_set_t wanted;
    CPU_ZERO(&wanted);
    for (int core = 0; core < GGML_MAX_N_THREADS && core < 64; ++core) {
        const jlong bit = static_cast<jlong>(1) << core;
        if (fast & bit) { usable |= bit; ++cores; CPU_SET(core, &wanted); }
    }
    if (cores < threads) return 0;
    bool pinned = sched_setaffinity(0, sizeof(wanted), &wanted) == 0;
    cpu_set_t got;
    if (pinned) pinned = sched_getaffinity(0, sizeof(got), &got) == 0 && core_bits(got) == usable;
    sched_setaffinity(0, sizeof(allowed), &allowed);
    return pinned ? usable : 0;
}

// The threads an unpinned pool gets: its workers inherit this thread's cores, and more spinning workers than cores
// leave every graph's barrier waiting on one that isn't running (4 on cpu 4-6: an 11 s clip took 230 s, #3037).
static int unpinned_threads(int threads) {
    cpu_set_t mine;
    if (sched_getaffinity(0, sizeof(mine), &mine) != 0) return threads;
    const int cores = CPU_COUNT(&mine);
    return cores > 0 ? std::min(threads, cores) : threads;
}

extern "C" JNIEXPORT jlong JNICALL
Java_ly_anoma_hex_speech_Whisper_usableCores(JNIEnv *, jobject, jlong fast, jint threads) { return pool_mask(fast, threads); }

// The cores the model's pool is kept on: -1 for no pool, 0 for any core.
extern "C" JNIEXPORT jlong JNICALL
Java_ly_anoma_hex_speech_Whisper_poolCores(JNIEnv *, jobject, jlong handle) {
    std::lock_guard<std::mutex> hold(pools_lock);
    auto found = pools.find(reinterpret_cast<whisper_context *>(handle));
    return found == pools.end() ? -1 : found->second.mask;
}

// Keeps a pool of [threads] worker threads with the model for its decodes, or none (0), ggml's default. A pool of
// another size or cores is replaced. ggml's default pool parameters, as its per-graph pools use: only reuse, the cores
// and the paused start differ. [fast] has a bit per core to keep them on (0: no restriction); a short encode then stays
// off the little ones instead of being one of them.
extern "C" JNIEXPORT jboolean JNICALL
Java_ly_anoma_hex_speech_Whisper_threadPool(JNIEnv *, jobject, jlong handle, jint threads, jlong fast) {
    auto ctx = reinterpret_cast<whisper_context *>(handle);
    if (!ctx) return false;
    const jlong mask = threads > 0 ? pool_mask(fast, threads) : 0;
    if (threads > 0 && mask == 0) threads = unpinned_threads(threads);
    {
        std::lock_guard<std::mutex> hold(pools_lock);
        auto found = pools.find(ctx);
        if (found != pools.end() && found->second.count == threads && found->second.mask == mask) return true;
    }
    drop_pool(ctx);
    if (threads <= 0) return true;
    auto params = ggml_threadpool_params_default(threads);
    // Started paused: the workers sleep as soon as they start instead of polling for a graph, and the first decode's
    // kickoff is where ggml applies the mask to the thread that runs it (ggml-cpu.c: kickoff resumes a paused pool), then
    // hexspeech's decode puts that thread's own affinity back. Not strict, as ggml's per-graph pools are: every worker
    // may use every core of the mask.
    params.paused = true;
    for (int core = 0; core < GGML_MAX_N_THREADS && core < 64; ++core)
        if (mask & (static_cast<jlong>(1) << core)) params.cpumask[core] = true;
    auto pool = ggml_threadpool_new(&params);
    if (!pool) return false;
    std::lock_guard<std::mutex> hold(pools_lock);
    pools[ctx] = {pool, threads, mask};
    return true;
}

// The last decode's timings, for Labs › Diagnostics: microseconds spent sampling, in the encoder, decoding one token at a
// time, decoding in batches and on the prompt, then the encoder window (0: the full 30 s), the thread count, the cores
// its threads were kept on (-1: no pool, new threads every graph; 0: kept on any core; else a bit per core, read back from
// the decoding thread, so a refused pinning reads as any core), then, when a prompted decode was decoded again without
// its prompt, the microseconds that took (0: never; the times before it include both decodes) and why (againWhy).
// Written by decode and read by timings, both under SpeechModel's lock.
static jlong last[10];

extern "C" JNIEXPORT jlongArray JNICALL
Java_ly_anoma_hex_speech_Whisper_timings(JNIEnv *env, jobject) {
    auto result = env->NewLongArray(10);
    if (result) env->SetLongArrayRegion(result, 0, 10, last);
    return result;
}

// Whether a decode up to 30 s (one window, no timestamps) repeated itself: it never wrote end-of-text, stopping at its
// token limit instead, and its last 32 tokens fail Whisper's own repetition test (whisper_sequence_score: their entropy
// under entropy_thold, 2.4; the same test already sends such a decode to its retry at 0.2). Ten "Sarah is at the station"
// in a row repeat a few tokens: entropy 1.6. Speech that runs into the limit (a dense language near 30 s) has
// end-of-text missing too, but 32 tokens of real sentences hold far more distinct ones, so it never fails both.
static bool repeated(whisper_context *ctx, whisper_state *state, float entropy_thold) {
    std::vector<whisper_token> tokens;
    for (int i = 0; i < whisper_full_n_segments_from_state(state); ++i)
        for (int j = 0; j < whisper_full_n_tokens_from_state(state, i); ++j)
            tokens.push_back(whisper_full_get_token_id_from_state(state, i, j));
    if (tokens.size() <= 32 || tokens.back() == whisper_token_eot(ctx)) return false;
    std::vector<whisper_token> tail(tokens.end() - 32, tokens.end());
    std::sort(tail.begin(), tail.end());
    double entropy = 0;
    for (size_t i = 0; i < tail.size();) {
        size_t j = i;
        while (j < tail.size() && tail[j] == tail[i]) ++j;
        const double p = static_cast<double>(j - i) / tail.size();
        entropy -= p * std::log(p);
        i = j;
    }
    return entropy < entropy_thold;
}

// A text's words, to compare a transcript with its prompt: ASCII letters lowercased, ASCII punctuation and spaces one
// break, other bytes (UTF-8) as they are.
static std::string spoken(const std::string &text) {
    std::string words;
    bool gap = false;
    for (const unsigned char c : text) {
        if (c < 0x80 && !std::isalnum(c)) { gap = true; continue; }
        if (gap && !words.empty()) words += ' ';
        gap = false;
        words += static_cast<char>(c < 0x80 ? std::tolower(c) : c);
    }
    return words;
}

// Whether a text's words are one phrase of three or more words written two or more times, and nothing else.
static bool onePhraseAgain(const std::string &words, int *count) {
    std::vector<std::string> w;
    for (size_t at = 0; at < words.size();) {
        const size_t end = std::min(words.find(' ', at), words.size());
        w.push_back(words.substr(at, end - at));
        at = end + 1;
    }
    *count = static_cast<int>(w.size());
    for (size_t p = 3; p * 2 <= w.size(); ++p) {
        if (w.size() % p) continue;
        bool same = true;
        for (size_t i = p; same && i < w.size(); ++i) same = w[i] == w[i - p];
        if (same) return true;
    }
    return false;
}

// Why a prompted decode up to 30 s (it had speech: MicCapture sends none without) is decoded again without the prompt:
// 0 it isn't; 1 it repeated itself; 2 it wrote nothing; 3 it wrote only the prompt, two or more terms, which Whisper
// returns for audio it can't read (the desktop app drops that echo, gguf_session.rs drop_prompt_echo); one term alone is
// kept, as saying the one listed name must still write it; 4 it wrote one phrase of three or more words two or more times
// and nothing else, more words than the clip holds (over 4 a second of it), as "Sarah is at the station" twice in 2.2 s.
// A sentence a person said twice takes the time of both: 40 such clips ran 1.9-2.9 words a second, and the prompt's
// copies 4.4-5.6, so a real repeat is never decoded again (which, unprompted, could write it once).
static jlong againWhy(whisper_context *ctx, whisper_state *state, const std::string &prompt, float entropy_thold, int count) {
    if (repeated(ctx, state, entropy_thold)) return 1;
    std::string text;
    for (int i = 0; i < whisper_full_n_segments_from_state(state); ++i) text += whisper_full_get_segment_text_from_state(state, i);
    const std::string words = spoken(text);
    if (words.empty()) return 2;
    if (prompt.find(',') != std::string::npos && words == spoken(prompt)) return 3;
    int n = 0;
    if (onePhraseAgain(words, &n) && n * 16000 > 4 * count) return 4;
    return 0;
}

// The name of the file this code was loaded from (libhexspeech.so, or another build of it), for Labs › Diagnostics.
extern "C" JNIEXPORT jstring JNICALL
Java_ly_anoma_hex_speech_Whisper_library(JNIEnv *env, jobject) {
    Dl_info info{};
    if (!dladdr(reinterpret_cast<void *>(&Java_ly_anoma_hex_speech_Whisper_library), &info) || !info.dli_fname) return nullptr;
    const char *name = std::strrchr(info.dli_fname, '/');
    return env->NewStringUTF(name ? name + 1 : info.dli_fname);
}

// What the last decode chose with Labs › Automatic listens for the language: the candidate it wrote in (0, the keyboard's
// language, 1 or 2; -1 when it had one language, so nothing was chosen), the choice's microseconds, then each candidate's
// probability among them in millionths (-1 past the last). Written by decode and read by language, both under
// SpeechModel's lock.
static jlong chosen[5] = {-1, 0, -1, -1, -1};

extern "C" JNIEXPORT jlongArray JNICALL
Java_ly_anoma_hex_speech_Whisper_language(JNIEnv *env, jobject) {
    auto result = env->NewLongArray(5);
    if (result) env->SetLongArrayRegion(result, 0, 5, chosen);
    return result;
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
    // The dictation language, or the languages Labs › Automatic listens for the language chooses among, comma-separated,
    // the keyboard's first (at most three of Hex's: "en,es,no").
    const char *code = env->GetStringUTFChars(language, nullptr);
    if (!code) return nullptr;
    std::string codes(code);
    env->ReleaseStringUTFChars(language, code);
    std::vector<std::string> languages;
    for (size_t at = 0; languages.size() < 3;) {
        const size_t comma = std::min(codes.find(',', at), codes.size());
        languages.push_back(codes.substr(at, comma - at));
        if (comma == codes.size()) break;
        at = comma + 1;
    }
    params.language = languages[0].c_str();
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
    // Labs › Automatic listens for the language: with more than one candidate, whisper-language-choice.patch reads the
    // language from this decode's own first window (one decoder step, about 3 ms on the host, where whisper.cpp's own
    // detection encodes a second, full window: about 2 s) and keeps the keyboard's unless another is at least 99% likely
    // among them, or 95% from 4 s of audio. On 2,466 cuts of 1.5-10 s in 14 languages the most a wrong language reached
    // was 0.98 below 3 s, 0.90 at 4 s and 0.56 from 5 s (benchmark/README.md). Whisper calls short Bokmål Nynorsk, so
    // "no" counts both; Danish and Swedish are never candidates. Only Spanish is translated; the others are transcribed.
    std::vector<int> ids, of;
    for (size_t i = 0; languages.size() > 1 && i < languages.size(); ++i) {
        for (const char *alias : {languages[i].c_str(), languages[i] == "no" ? "nn" : nullptr}) {
            const int id = alias ? whisper_lang_id(alias) : -1;
            if (id >= 0) { ids.push_back(id); of.push_back(static_cast<int>(i)); }
        }
    }
    const auto listen = [&](whisper_state *s) {
        if (!ids.empty()) hex_whisper_state_set_languages(s, ids.data(), of.data(), static_cast<int>(ids.size()),
                                                          count < 4 * 16000 ? 0.99f : 0.95f, translate ? whisper_lang_id("es") : -1);
    };
    listen(state.get());
    ggml_threadpool *pool = nullptr;
    jlong mask = 0;
    {
        std::lock_guard<std::mutex> hold(pools_lock);
        auto found = pools.find(ctx);
        if (found != pools.end()) {
            pool = found->second.threads;
            mask = found->second.mask;
            params.n_threads = std::min(params.n_threads, found->second.count);
        }
    }
    // ggml gives the thread that computes a graph the pool's own mask too (ggml-cpu.c: ggml_graph_compute_kickoff
    // applies workers[0]'s). That is this thread, which for dictation also loads models and runs Parakeet's graphs,
    // and for the speed test also times the no-pool run: keep what it had and put it back once the decode ends. A
    // masked pool runs only when that is saved, else this decode keeps ggml's own threads and no pinning.
    cpu_set_t mine;
    const bool attach = pool && (!mask || sched_getaffinity(0, sizeof(mine), &mine) == 0);
    // The pool outlives the state, which never frees it. A decode stopped by abort ends between graphs, so the pool is
    // idle and the next graph resets its abort flag.
    if (attach) hex_whisper_state_set_threadpool(state.get(), pool);
#ifndef NDEBUG
    const auto started = std::chrono::steady_clock::now();
#endif
    int status = whisper_full_with_state(ctx, state.get(), params, pcm.data(), count);
    // A prompt in front of a short clip can make Whisper repeat the clip's sentence to its token limit ("Sarah is at the
    // station" ten times in 2.2 s, prompted "stadium", the word custom vocabulary or Labs › Learn from my fixes adds),
    // and its retry at 0.2 keeps the prompt and repeats too; or write it twice, nothing, or only the prompt. Such a decode
    // runs once more without the prompt, in a fresh state, and that text is used: the dictation as it would have come out
    // with no words added. Only those decodes pay for it; past 30 s, with timestamps, none is checked.
    std::unique_ptr<whisper_state, decltype(&whisper_free_state)> again(nullptr, whisper_free_state);
    jlong again_us = 0;
    const jlong why = status == 0 && params.initial_prompt && params.no_timestamps ? againWhy(ctx, state.get(), hint, params.entropy_thold, count) : 0;
    if (why != 0) {
        again.reset(whisper_init_state(ctx));
        if (again) {
            if (attach) hex_whisper_state_set_threadpool(again.get(), pool);
            // The same languages: the choice reads only the audio, so it comes out as the first decode's did.
            listen(again.get());
            params.initial_prompt = nullptr;
            const auto from = std::chrono::steady_clock::now();
            status = whisper_full_with_state(ctx, again.get(), params, pcm.data(), count);
            again_us = std::max<jlong>(1, std::chrono::duration_cast<std::chrono::microseconds>(std::chrono::steady_clock::now() - from).count());
        }
    }
    whisper_state *result_state = again ? again.get() : state.get();
    // Paused workers wait on a condition variable until the next graph instead of polling.
    if (attach) ggml_threadpool_pause(pool);
    // Where this decode's threads were kept, for Labs › Diagnostics: read back from this thread, which ggml pinned to the
    // pool's mask at the first graph unless the kernel refused (then it reads as any core).
    jlong kept = attach ? 0 : -1;
    if (attach && mask) {
        cpu_set_t now_on;
        if (sched_getaffinity(0, sizeof(now_on), &now_on) == 0) {
            jlong on = 0;
            for (int core = 0; core < 64; ++core) if (CPU_ISSET(core, &now_on)) on |= static_cast<jlong>(1) << core;
            if (on != 0 && (on & ~mask) == 0) kept = on;
        }
        sched_setaffinity(0, sizeof(mine), &mine);
    }
    whisper_timings spent{};
    hex_whisper_state_timings(state.get(), &spent);
    if (again) {
        whisper_timings more{};
        hex_whisper_state_timings(again.get(), &more);
        spent.sample_ms += more.sample_ms; spent.encode_ms += more.encode_ms; spent.decode_ms += more.decode_ms;
        spent.batchd_ms += more.batchd_ms; spent.prompt_ms += more.prompt_ms;
    }
    const jlong now[10] = {static_cast<jlong>(spent.sample_ms * 1000), static_cast<jlong>(spent.encode_ms * 1000),
                          static_cast<jlong>(spent.decode_ms * 1000), static_cast<jlong>(spent.batchd_ms * 1000),
                          static_cast<jlong>(spent.prompt_ms * 1000), params.audio_ctx, params.n_threads, kept, again_us,
                           again ? why : 0};
    std::copy(std::begin(now), std::end(now), std::begin(last));
#ifndef NDEBUG
    // Diagnostics identify the engine and task only; no audio or recognized text is logged.
    const auto ms = std::chrono::duration_cast<std::chrono::milliseconds>(std::chrono::steady_clock::now() - started).count();
    __android_log_print(ANDROID_LOG_DEBUG, "HexSpeech", "%s %s %s in %lld ms, %d samples, %d threads, window %d, max %d tokens%s",
                        small ? "small" : "tiny", translate ? "translate" : "transcribe",
                        status == 0 ? "complete" : abandoned.load() == job ? "abandoned" : "failed", static_cast<long long>(ms),
                        count, params.n_threads, params.audio_ctx, params.max_tokens, again ? ", again without the prompt" : "");
#endif
    {
        float p[3] = {-1, -1, -1};
        int64_t us = 0;
        const int which = languages.size() > 1 ? hex_whisper_state_language(result_state, p, 3, &us) : -1;
        chosen[0] = which;
        chosen[1] = us;
        for (int i = 0; i < 3; ++i)
            chosen[2 + i] = which >= 0 && i < static_cast<int>(languages.size()) ? std::lround(p[i] * 1e6) : -1;
    }
    if (status != 0) return nullptr;
    std::string text;
    for (int i = 0; i < whisper_full_n_segments_from_state(result_state); ++i)
        text += whisper_full_get_segment_text_from_state(result_state, i);
    // JNI's modified UTF-8 is not standard UTF-8 (notably for supplementary code points).
    auto result = env->NewByteArray(static_cast<jsize>(text.size()));
    if (result) env->SetByteArrayRegion(result, 0, static_cast<jsize>(text.size()),
                                        reinterpret_cast<const jbyte *>(text.data()));
    return result;
}
