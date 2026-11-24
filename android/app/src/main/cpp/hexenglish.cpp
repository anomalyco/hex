#include <jni.h>

#include <algorithm>
#include <atomic>
#include <string>
#include <vector>

#include "transcribe.h"

// Faster English dictation (Labs): NVIDIA's Parakeet TDT-CTC 110M through transcribe.cpp, on the CPU only, from the
// file ModelStore verified. The library is loaded only while that switch is on.

// The dictation the person discarded (✕, or Home's Cancel), by MicCapture's job number, as in hexspeech.cpp.
// transcribe.cpp asks before each stretch of speech it decodes, so a discarded dictation ends there.
static std::atomic<jlong> abandoned{0};
// Why the last load failed (transcribe_status), so the app can tell running out of memory from an unusable file.
static std::atomic<int> failure{TRANSCRIBE_OK};

extern "C" JNIEXPORT jlong JNICALL
Java_ly_anoma_hex_speech_Parakeet_load(JNIEnv *env, jobject, jstring file, jint threads) {
    // Nothing transcribe.cpp logs reaches logcat: some of it names words it heard.
    static const bool quiet = [] { transcribe_log_set([](transcribe_log_level, const char *, void *) {}, nullptr); return true; }();
    (void) quiet;
    const char *path = env->GetStringUTFChars(file, nullptr);
    if (!path) { failure.store(TRANSCRIBE_ERR_OOM); return 0; }
    transcribe_model_load_params load;
    transcribe_model_load_params_init(&load);
    load.backend = TRANSCRIBE_BACKEND_CPU;
    transcribe_session_params options;
    transcribe_session_params_init(&options);
    options.n_threads = std::clamp<int>(threads, 1, 4);
    transcribe_session *session = nullptr;
    const transcribe_status status = transcribe_open(path, &load, &options, &session);
    env->ReleaseStringUTFChars(file, path);
    failure.store(status);
    return status == TRANSCRIBE_OK ? reinterpret_cast<jlong>(session) : 0;
}

extern "C" JNIEXPORT jboolean JNICALL
Java_ly_anoma_hex_speech_Parakeet_outOfMemory(JNIEnv *, jobject) { return failure.load() == TRANSCRIBE_ERR_OOM; }

extern "C" JNIEXPORT void JNICALL
Java_ly_anoma_hex_speech_Parakeet_free(JNIEnv *, jobject, jlong handle) {
    transcribe_close(reinterpret_cast<transcribe_session *>(handle));
}

extern "C" JNIEXPORT void JNICALL
Java_ly_anoma_hex_speech_Parakeet_abort(JNIEnv *, jobject, jlong job) { abandoned.store(job); }

extern "C" JNIEXPORT jbyteArray JNICALL
Java_ly_anoma_hex_speech_Parakeet_decode(JNIEnv *env, jobject, jlong handle, jshortArray samples, jint count,
                                          jobjectArray vocabulary, jlong job) {
    auto session = reinterpret_cast<transcribe_session *>(handle);
    if (!session || count < 1 || count > env->GetArrayLength(samples)) return nullptr;
    std::vector<float> pcm(count);
    jshort page[2048];
    for (int i = 0; i < count; i += 2048) {
        const int n = std::min(2048, count - i);
        env->GetShortArrayRegion(samples, i, n, page);
        if (env->ExceptionCheck()) return nullptr;
        for (int j = 0; j < n; ++j) pcm[i + j] = page[j] / 32768.0f;
    }
    // Custom vocabulary: transcribe.cpp rescores Parakeet's reading of these terms, in priority order.
    std::vector<std::string> terms;
    const jsize n = vocabulary ? env->GetArrayLength(vocabulary) : 0;
    for (jsize i = 0; i < n; ++i) {
        auto term = static_cast<jstring>(env->GetObjectArrayElement(vocabulary, i));
        if (!term) continue;
        const char *chars = env->GetStringUTFChars(term, nullptr);
        if (chars) { terms.emplace_back(chars); env->ReleaseStringUTFChars(term, chars); }
        env->DeleteLocalRef(term);
        if (env->ExceptionCheck()) return nullptr;
    }
    std::vector<const char *> words;
    for (const auto &term : terms) words.push_back(term.c_str());
    transcribe_run_params params;
    transcribe_run_params_init(&params);
    params.vocabulary = words.empty() ? nullptr : words.data();
    params.n_vocabulary = static_cast<int32_t>(words.size());
    transcribe_set_abort_callback(session, [](void *mine) { return abandoned.load() == *static_cast<const jlong *>(mine); }, &job);
    const transcribe_status status = transcribe_run(session, pcm.data(), count, &params);
    transcribe_set_abort_callback(session, nullptr, nullptr);
    if (status != TRANSCRIBE_OK) return nullptr;
    const char *text = transcribe_full_text(session);
    if (!text) return nullptr;
    const std::string bytes(text);
    // JNI's modified UTF-8 is not standard UTF-8 (notably for supplementary code points).
    auto result = env->NewByteArray(static_cast<jsize>(bytes.size()));
    if (result) env->SetByteArrayRegion(result, 0, static_cast<jsize>(bytes.size()), reinterpret_cast<const jbyte *>(bytes.data()));
    return result;
}
