// Android/arm64 benchmark. Corpus labels are used only for choosing the input language.
#include <algorithm>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <fstream>
#include <iostream>
#include <string>
#include <sys/resource.h>
#include <vector>
#ifdef PARAKEET
#include "sherpa-onnx/c-api/c-api.h"
#else
#include <whisper.h>
#endif

static double ms(clockid_t clock) {
    timespec t{}; clock_gettime(clock, &t); return t.tv_sec*1000.0+t.tv_nsec/1e6;
}
int main(int argc, char **argv) {
    if (argc != 4) return 2; // model path, thread count, TSV with id + language
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
#else
    auto opts=whisper_context_default_params();opts.use_gpu=false;
    auto ctx=whisper_init_from_file_with_params(model.c_str(),opts);
#endif
    if (!ctx) return 3;
    auto load=ms(CLOCK_MONOTONIC)-start;
    std::ifstream list(argv[3]); std::string id,lang;
    while(list>>id>>lang) {
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
#else
        auto p=whisper_full_default_params(WHISPER_SAMPLING_GREEDY);
        if (getenv("HEX_SHORT_WINDOW") && audio.size() <= 8*16000) p.audio_ctx=512;
        p.n_threads=threads;p.language=lang.c_str();p.no_context=true;p.no_timestamps=true;p.suppress_nst=true;
        p.print_progress=p.print_special=p.print_timestamps=p.print_realtime=false;
        if (whisper_full(ctx,p,audio.data(),audio.size())!=0) return 6;
        for(int i=0;i<whisper_full_n_segments(ctx);i++) text+=whisper_full_get_segment_text(ctx,i);
#endif
        for(char &c:text) if(c=='\t'||c=='\n'||c=='\r') c=' ';
        rusage u{};getrusage(RUSAGE_SELF,&u);
        printf("%s\t%.1f\t%.1f\t%.1f\t%ld\t%s\n",id.c_str(),ms(CLOCK_MONOTONIC)-wall,ms(CLOCK_PROCESS_CPUTIME_ID)-cpu,load,u.ru_maxrss,text.c_str());fflush(stdout);
    }
#ifdef PARAKEET
    SherpaOnnxDestroyOfflineRecognizer(ctx);
#else
    whisper_free(ctx);
#endif
}
