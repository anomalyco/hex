# The Linux release build environment (scripts/build-linux-release.sh):
# Debian 12, so the executable and packages need only glibc 2.36 and
# libstdc++ from GCC 12 (Debian 12, Ubuntu 23.04, Fedora 37, and newer;
# issue #1026). Checked by .github/release/check-glibc-floor.sh.
#
# Debian 12's Vulkan headers are too old for ggml's Vulkan backend and its
# glslc lacks the cooperative-matrix, integer-dot-product, and bfloat16 shader
# extensions, so both are built from Vulkan SDK 1.4.341's sources. They are
# build tools only; the executable still loads the system's libvulkan.so.1.
FROM debian:12@sha256:d74df7598108a173a621eda4d58a2adbf850b2d9a9d81bc2cca1a6c21174a880

RUN apt-get update \
  && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
    ca-certificates clang cmake curl file git jq libasound2-dev libfontconfig1-dev \
    libfreetype-dev libopenblas-dev libssl-dev libvulkan-dev libx11-xcb-dev \
    libxcursor-dev libxi-dev libxkbcommon-x11-dev libxrandr-dev make ninja-build \
    openssl pkg-config python3 \
  && rm -rf /var/lib/apt/lists/*

RUN git clone --depth 1 --branch v1.4.341 https://github.com/KhronosGroup/Vulkan-Headers /tmp/vulkan-headers \
  && cmake -S /tmp/vulkan-headers -B /tmp/vulkan-headers/build -G Ninja \
  && cmake --install /tmp/vulkan-headers/build \
  && rm -rf /tmp/vulkan-headers

RUN git clone --depth 1 --branch v2026.1 https://github.com/google/shaderc /tmp/shaderc \
  && cd /tmp/shaderc \
  && ./utils/git-sync-deps \
  && cmake -S . -B build -G Ninja -DCMAKE_BUILD_TYPE=Release -DSHADERC_SKIP_TESTS=ON \
    -DSHADERC_SKIP_EXAMPLES=ON -DSHADERC_SKIP_COPYRIGHT_CHECK=ON -DSPIRV_SKIP_TESTS=ON \
    -DENABLE_GLSLANG_BINARIES=OFF \
  && cmake --build build --target glslc_exe \
  && install -m755 build/glslc/glslc /usr/local/bin/glslc \
  && cmake -S third_party/spirv-headers -B build/spirv-headers -G Ninja \
  && cmake --install build/spirv-headers \
  && cd / && rm -rf /tmp/shaderc
