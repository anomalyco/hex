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
# Only the JNI entry points (JNIEXPORT) are exported; unused code, such as ggml's backend loader, is dropped.
set(CMAKE_C_VISIBILITY_PRESET hidden)
set(CMAKE_CXX_VISIBILITY_PRESET hidden)
set(CMAKE_VISIBILITY_INLINES_HIDDEN ON)
add_compile_options(-fvisibility=hidden -ffunction-sections -fdata-sections)
# A keyboard's process should not carry code that forks, executes programs, sets a ptracer or loads shared objects from
# a path: the patch keeps ggml_abort's backtrace in process (stock ggml forks gdb or lldb). A bump it no longer fits fails.
# whisper-state-timings.patch reads a decode's encoder and decoder time from its own state, for Labs › Diagnostics.
find_program(PATCH_EXECUTABLE patch REQUIRED)
FetchContent_Declare(whisper_cpp
    URL https://codeload.github.com/ggml-org/whisper.cpp/tar.gz/d1be6fde11ac6e0407606b4e42fe72d34add8037
    URL_HASH SHA256=645445da884df6dbd52b30f82d7be7c01d1767c938714e477e786b88d529e9f4
    TLS_VERIFY ON
    PATCH_COMMAND ${PATCH_EXECUTABLE} -p1 -N -t -F0 -i ${CMAKE_CURRENT_LIST_DIR}/ggml-no-debugger.patch
          COMMAND ${PATCH_EXECUTABLE} -p1 -N -t -F0 -i ${CMAKE_CURRENT_LIST_DIR}/whisper-state-timings.patch)
FetchContent_MakeAvailable(whisper_cpp)

add_library(hexspeech SHARED hexspeech.cpp)
target_compile_features(hexspeech PRIVATE cxx_std_17)
target_link_libraries(hexspeech PRIVATE whisper android log)
target_link_options(hexspeech PRIVATE -Wl,--gc-sections -Wl,--exclude-libs,ALL)
add_custom_command(TARGET hexspeech POST_BUILD VERBATIM
    COMMAND ${CMAKE_COMMAND} -DNM=${CMAKE_NM} -DLIB=$<TARGET_FILE:hexspeech> -P ${CMAKE_CURRENT_LIST_DIR}/check-imports.cmake)
