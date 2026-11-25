// Android/arm64 benchmark. Corpus labels are used only for choosing the input language.
#include <algorithm>
#include <cctype>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <fstream>
#include <iostream>
#include <sstream>
#include <string>
#include <sys/resource.h>
#include <vector>
#ifdef PARAKEET
#include "sherpa-onnx/c-api/c-api.h"
#elif defined(TRANSCRIBE)
#include "transcribe.h"
#else
#include <whisper.h>
#endif

static double ms(clockid_t clock) {
    timespec t{}; clock_gettime(clock, &t); return t.tv_sec*1000.0+t.tv_nsec/1e6;
}
#if !defined(PARAKEET) && !defined(TRANSCRIBE)
// hexspeech.cpp's repeated(): no end-of-text, and the last 32 tokens fail Whisper's entropy test.
static bool repeated(whisper_context *ctx, whisper_state *state, float thold) {
    std::vector<whisper_token> t;
    for (int i=0;i<whisper_full_n_segments_from_state(state);i++)
        for (int j=0;j<whisper_full_n_tokens_from_state(state,i);j++) t.push_back(whisper_full_get_token_id_from_state(state,i,j));
    if (t.size()<=32 || t.back()==whisper_token_eot(ctx)) return false;
    std::vector<whisper_token> tail(t.end()-32,t.end()); std::sort(tail.begin(),tail.end());
    double e=0;
    for (size_t i=0;i<tail.size();) { size_t j=i; while(j<tail.size()&&tail[j]==tail[i]) j++; double p=double(j-i)/tail.size(); e-=p*std::log(p); i=j; }
    return e<thold;
}
// hexspeech.cpp's spoken() and againWhy(): repeated, no words, or only the prompt's two or more terms.
static std::string spoken(const std::string &text) {
    std::string w; bool gap=false;
    for (const unsigned char c:text) {
        if (c<0x80 && !std::isalnum(c)) { gap=true; continue; }
        if (gap && !w.empty()) w+=' ';
        gap=false; w+=static_cast<char>(c<0x80?std::tolower(c):c);
    }
    return w;
}
static bool again(whisper_context *ctx, whisper_state *state, const std::string &prompt, float thold, int count) {
    if (repeated(ctx,state,thold)) return true;
    std::string text; for (int i=0;i<whisper_full_n_segments_from_state(state);i++) text+=whisper_full_get_segment_text_from_state(state,i);
    const std::string w=spoken(text);
    if (w.empty() || (prompt.find(',')!=std::string::npos && w==spoken(prompt))) return true;
    std::vector<std::string> ws; std::istringstream in(w); for (std::string x; in>>x;) ws.push_back(x);
    for (size_t p=3;p*2<=ws.size();p++) {
        if (ws.size()%p) continue;
        bool same=true; for (size_t i=p;same&&i<ws.size();i++) same=ws[i]==ws[i-p];
        if (same) return (long)ws.size()*16000>4L*count;
    }
    return false;
}
#endif
int main(int argc, char **argv) {
    if (argc != 4) return 2; // model path, thread count, list: id, language and (Whisper) an optional prompt per line
    std::string model=argv[1]; int threads=atoi(argv[2]);
    auto start=ms(CLOCK_MONOTONIC);
#ifdef PARAKEET
    std::string enc=model+"/encoder.int8.onnx",dec=model+"/decoder.int8.onnx",join=model+"/joiner.int8.onnx",tokens=model+"/tokens.txt";
    SherpaOnnxOfflineRecognizerConfig cfg{};
    cfg.feat_config.sample_rate=16000; cfg.feat_config.feature_dim=128;
    cfg.model_config.transducer={enc.c_str(),dec.c_str(),join.c_str()};
    cfg.model_config.tokens=tokens.c_str(); cfg.model_config.provider="cpu"; cfg.model_config.num_threads=threads;
    cfg.decoding_method="greedy_search";
    auto ctx=SherpaOnnxCreateOfflineRecognizer(&cfg);
#elif defined(TRANSCRIBE)
    // Faster English dictation's settings (app/src/main/cpp/hexenglish.cpp): CPU only, the app's thread count.
    transcribe_model_load_params lp; transcribe_model_load_params_init(&lp); lp.backend=TRANSCRIBE_BACKEND_CPU;
    transcribe_session_params sp; transcribe_session_params_init(&sp); sp.n_threads=threads;
    transcribe_session *ctx=nullptr;
    if (transcribe_open(model.c_str(),&lp,&sp,&ctx)!=TRANSCRIBE_OK) ctx=nullptr;
#else
    auto opts=whisper_context_default_params();opts.use_gpu=false;
    auto ctx=whisper_init_from_file_with_params(model.c_str(),opts);
#endif
    if (!ctx) return 3;
    auto load=ms(CLOCK_MONOTONIC)-start;
    std::ifstream list(argv[3]); std::string line;
    while(std::getline(list,line)) {
        // The rest of the line after id and language is the prompt, the custom vocabulary or learned words as the app
        // joins them ("Sarah, stadium"); none on the corpus and long lists.
        std::istringstream fields(line); std::string id,lang,prompt;
        if (!(fields>>id>>lang)) continue;
        std::getline(fields>>std::ws,prompt);
        std::ifstream f("wavs/"+id+".wav",std::ios::binary);
        char hdr[44]; f.read(hdr,44);
        if (!f || std::memcmp(hdr,"RIFF",4) || std::memcmp(hdr+8,"WAVEfmt ",8) ||
            std::memcmp(hdr+36,"data",4) || hdr[20]!=1 || hdr[21] || hdr[22]!=1 || hdr[23] ||
            hdr[34]!=16 || hdr[35]) return 4;
        uint32_t rate, bytes; std::memcpy(&rate,hdr+24,4); std::memcpy(&bytes,hdr+40,4);
        if (rate!=16000 || !bytes || bytes%sizeof(int16_t) || bytes>60*rate*sizeof(int16_t)) return 4;
        std::vector<int16_t> shorts(bytes/sizeof(int16_t)); f.read((char*)shorts.data(),bytes);
        if (!f) return 5;
        std::vector<float> audio; audio.reserve(shorts.size());
        for (auto x:shorts) audio.push_back(x/32768.0f);
        auto wall=ms(CLOCK_MONOTONIC),cpu=ms(CLOCK_PROCESS_CPUTIME_ID);
        std::string text;
#ifdef PARAKEET
        auto stream=SherpaOnnxCreateOfflineStream(ctx);
        SherpaOnnxAcceptWaveformOffline(stream,16000,audio.data(),audio.size());
        SherpaOnnxDecodeOfflineStream(ctx,stream);
        auto result=SherpaOnnxGetOfflineStreamResult(stream); if (result) text=result->text;
        SherpaOnnxDestroyOfflineRecognizerResult(result);SherpaOnnxDestroyOfflineStream(stream);
#elif defined(TRANSCRIBE)
        transcribe_run_params rp; transcribe_run_params_init(&rp);
        if (transcribe_run(ctx,audio.data(),(int)audio.size(),&rp)!=TRANSCRIBE_OK) return 6;
        text=transcribe_full_text(ctx);
#else
        auto p=whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
        if (getenv("HEX_SHORT_WINDOW") && audio.size() <= 8*16000) p.audio_ctx=512;
        p.n_threads=threads;p.language=lang.c_str();p.no_context=true;p.no_timestamps=true;p.suppress_nst=true;
        if (getenv("HEX_APP") && audio.size()>30*16000) p.no_timestamps=false; // the app's long-audio decode
        p.print_progress=p.print_special=p.print_timestamps=p.print_realtime=false;
        if (!prompt.empty()) p.initial_prompt=prompt.c_str();
        // HEX_APP: the downloaded model exactly as app/src/main/cpp/hexspeech.cpp decodes it (the window sized to the
        // speech: up to 8 s plus 192 frames, at most 512, longer plus 640 until the full 1500; the token cap, one retry
        // at 0.2, a fresh state per clip). HEX_APP=full keeps Whisper's full window past 8 s, for the long-clip check.
        const char *appv=getenv("HEX_APP"); const bool app=appv!=nullptr; const int count=(int)audio.size();
        if (app) {
            const int speech=(int)std::ceil(count*50.0/16000);
            const int frames=count<=8*16000?std::min(512,(speech+192+63)/64*64):std::strcmp(appv,"full")?(speech+640+63)/64*64:1500;
            if (frames<1500) p.audio_ctx=frames;
            p.max_tokens=std::min(220,(int)std::ceil(std::min(count,30*16000)*15.0/16000)+16);
            p.greedy.best_of=1;p.temperature=-0.8f;p.temperature_inc=1.0f;
        }
        whisper_state *state=app?whisper_init_state(ctx):nullptr;
        if (app?whisper_full_with_state(ctx,state,p,audio.data(),count)!=0:whisper_full(ctx,p,audio.data(),count)!=0) return 6;
        // The app decodes a prompted clip up to 30 s that repeated itself, wrote one phrase twice in too little audio, nothing
        // or only the prompt again without the prompt, in a fresh state, and uses that (hexspeech.cpp's againWhy);
        // HEX_NO_GUARD=1 is the decode before, which kept them.
        if (app && p.initial_prompt && p.no_timestamps && !getenv("HEX_NO_GUARD") && again(ctx,state,prompt,p.entropy_thold,count)) {
            whisper_free_state(state); state=whisper_init_state(ctx); p.initial_prompt=nullptr;
            if (!state || whisper_full_with_state(ctx,state,p,audio.data(),count)!=0) return 6;
        }
        if (app) { for(int i=0;i<whisper_full_n_segments_from_state(state);i++) text+=whisper_full_get_segment_text_from_state(state,i); whisper_free_state(state); }
        else for(int i=0;i<whisper_full_n_segments(ctx);i++) text+=whisper_full_get_segment_text(ctx,i);
#endif
        for(char &c:text) if(c=='\t'||c=='\n'||c=='\r') c=' ';
        rusage u{};getrusage(RUSAGE_SELF,&u);
        printf("%s\t%.1f\t%.1f\t%.1f\t%ld\t%s\n",id.c_str(),ms(CLOCK_MONOTONIC)-wall,ms(CLOCK_PROCESS_CPUTIME_ID)-cpu,load,u.ru_maxrss,text.c_str());fflush(stdout);
    }
#ifdef PARAKEET
    SherpaOnnxDestroyOfflineRecognizer(ctx);
#elif defined(TRANSCRIBE)
    transcribe_close(ctx);
#else
    whisper_free(ctx);
#endif
}
