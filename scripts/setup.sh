#!/bin/sh
set -eu

# Downloads ONNX Runtime 1.23.2 (macOS arm64) for the Smart Turn experiment and
# Hex.app packaging. Linux and Windows builds do not use ONNX Runtime.
if [ "$(uname -s)" != Darwin ]; then
  exit 0
fi

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
dest="$root/target/onnxruntime"
dylib="$dest/libonnxruntime.1.23.2.dylib"
version=1.23.2
sha256=b4d513ab2b26f088c66891dbbc1408166708773d7cc4163de7bdca0e9bbb7856
url="https://github.com/microsoft/onnxruntime/releases/download/v$version/onnxruntime-osx-arm64-$version.tgz"

if [ -f "$dylib" ]; then
  exit 0
fi

mkdir -p "$dest"
tmp=$(mktemp -d "${TMPDIR:-/tmp}/hex-onnxruntime.XXXXXX")
trap 'rm -rf "$tmp"' EXIT

curl -fsSL "$url" -o "$tmp/onnxruntime.tgz"
actual=$(shasum -a 256 "$tmp/onnxruntime.tgz" | awk '{print $1}')
if [ "$actual" != "$sha256" ]; then
  echo "ONNX Runtime checksum mismatch: expected $sha256, got $actual" >&2
  exit 1
fi

tar -xzf "$tmp/onnxruntime.tgz" -C "$tmp"
cp "$tmp/onnxruntime-osx-arm64-$version/lib/libonnxruntime.$version.dylib" "$dylib"
