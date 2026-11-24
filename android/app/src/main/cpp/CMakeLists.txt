cmake_minimum_required(VERSION 3.22)
project(hexspeech LANGUAGES C CXX)

include(FetchContent)
include(ExternalProject)
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
find_program(PATCH_EXECUTABLE patch REQUIRED)
FetchContent_Declare(whisper_cpp
    URL https://codeload.github.com/ggml-org/whisper.cpp/tar.gz/d1be6fde11ac6e0407606b4e42fe72d34add8037
    URL_HASH SHA256=645445da884df6dbd52b30f82d7be7c01d1767c938714e477e786b88d529e9f4
    TLS_VERIFY ON
    PATCH_COMMAND ${PATCH_EXECUTABLE} -p1 -N -t -F0 -i ${CMAKE_CURRENT_LIST_DIR}/ggml-no-debugger.patch)
FetchContent_MakeAvailable(whisper_cpp)
# The second copy below builds Whisper and ggml again through this file, stopping here.
if(HEX_SPEECH_DEPS_ONLY)
    return()
endif()

function(hexspeech_library name)
    add_library(${name} SHARED hexspeech.cpp)
    target_compile_features(${name} PRIVATE cxx_std_17)
    target_link_libraries(${name} PRIVATE ${ARGN} android log)
    target_link_options(${name} PRIVATE -Wl,--gc-sections -Wl,--exclude-libs,ALL)
    add_custom_command(TARGET ${name} POST_BUILD VERBATIM
        COMMAND ${CMAKE_COMMAND} -DNM=${CMAKE_NM} -DLIB=$<TARGET_FILE:${name}> -P ${CMAKE_CURRENT_LIST_DIR}/check-imports.cmake)
endfunction()

# Runs on any arm64 processor.
hexspeech_library(hexspeech whisper)

# libhexspeech_v82: the same library with ggml's CPU code built for ARMv8.2 with the dot-product instructions
# (ggml's android_armv8.2_1 variant), which Whisper.kt loads only when the processor reports them. Whisper and ggml
# are built a second time by this file, from the same patched source, with only GGML_CPU_ARM_ARCH changed.
set(HEX_SPEECH_V82_ARCH armv8.2-a+dotprod CACHE STRING "GGML_CPU_ARM_ARCH of libhexspeech_v82")
set(v82 ${CMAKE_CURRENT_BINARY_DIR}/v82)
set(v82_archives ${v82}/libwhisper.a ${v82}/libggml.a ${v82}/libggml-cpu.a ${v82}/libggml-base.a)
set(v82_args -DHEX_SPEECH_DEPS_ONLY=ON -DGGML_CPU_ARM_ARCH=${HEX_SPEECH_V82_ARCH}
    -DFETCHCONTENT_SOURCE_DIR_WHISPER_CPP=${whisper_cpp_SOURCE_DIR} -DCMAKE_ARCHIVE_OUTPUT_DIRECTORY=${v82})
foreach(var CMAKE_TOOLCHAIN_FILE CMAKE_BUILD_TYPE CMAKE_MAKE_PROGRAM ANDROID_ABI ANDROID_PLATFORM ANDROID_NDK ANDROID_STL
        ANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES ANDROID_USE_LEGACY_TOOLCHAIN_FILE)
    if(DEFINED ${var})
        list(APPEND v82_args -D${var}=${${var}})
    endif()
endforeach()
ExternalProject_Add(hexspeech_v82_deps
    SOURCE_DIR ${CMAKE_CURRENT_LIST_DIR}
    BINARY_DIR ${v82}/build
    CMAKE_ARGS ${v82_args}
    BUILD_COMMAND ${CMAKE_COMMAND} --build <BINARY_DIR> --target whisper
    BUILD_ALWAYS ON
    INSTALL_COMMAND ""
    BUILD_BYPRODUCTS ${v82_archives})
hexspeech_library(hexspeech_v82 -Wl,--start-group ${v82_archives} -Wl,--end-group)
target_include_directories(hexspeech_v82 PRIVATE ${whisper_cpp_SOURCE_DIR}/include ${whisper_cpp_SOURCE_DIR}/ggml/include)
add_dependencies(hexspeech_v82 hexspeech_v82_deps)
