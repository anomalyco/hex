cmake_minimum_required(VERSION 3.22)
project(hexspeech LANGUAGES C CXX)

include(FetchContent)
set(BUILD_SHARED_LIBS OFF CACHE BOOL "Link Whisper and ggml into the JNI library" FORCE)
set(WHISPER_BUILD_TESTS OFF CACHE BOOL "")
set(WHISPER_BUILD_EXAMPLES OFF CACHE BOOL "")
set(WHISPER_BUILD_SERVER OFF CACHE BOOL "")
set(GGML_NATIVE OFF CACHE BOOL "")
set(GGML_OPENMP OFF CACHE BOOL "")
set(GGML_LLAMAFILE OFF CACHE BOOL "")
set(GGML_CPU_ARM_ARCH armv8-a CACHE STRING "")
set(CMAKE_POSITION_INDEPENDENT_CODE ON)
# Interactive decoding needs optimized kernels even in the debuggable APK.
add_compile_options($<$<CONFIG:Debug>:-O2>)
FetchContent_Declare(whisper_cpp
    URL https://codeload.github.com/ggml-org/whisper.cpp/tar.gz/d1be6fde11ac6e0407606b4e42fe72d34add8037
    URL_HASH SHA256=645445da884df6dbd52b30f82d7be7c01d1767c938714e477e786b88d529e9f4
    TLS_VERIFY ON)
FetchContent_MakeAvailable(whisper_cpp)

add_library(hexspeech SHARED hexspeech.cpp)
target_compile_features(hexspeech PRIVATE cxx_std_17)
target_link_libraries(hexspeech PRIVATE whisper android log)
