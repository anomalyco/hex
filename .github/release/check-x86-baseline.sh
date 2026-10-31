#!/usr/bin/env bash
# shellcheck disable=SC2034 # the patterns are read through ${!kind}
# check-x86-baseline.sh <executable> [<ggml lib>...]: fails if an x86_64
# release would crash on a CPU at Hex's floor (Ivy Bridge: SSE4.2, AVX, F16C;
# no AVX2, BMI2, FMA or AVX-512; issue #592).
#
# - The executable may hold no BMI2 instruction at all. Rust crates that
#   detect AVX2/FMA/AVX-512 at run time (blake3, rustfft, curve25519,
#   simdutf8, zune-jpeg) use those freely, so the executable is checked for
#   BMI2 only, which none of them use.
# - ggml's static CPU libraries have no run-time dispatch in this build, so
#   they may hold no AVX2, BMI2, FMA or AVX-512 instruction.
#
# OBJDUMP names the disassembler (default objdump; llvm-objdump for COFF).
. "$(dirname "$0")/lib.sh"

objdump=${OBJDUMP:-objdump}
bmi2='\b(shlx|shrx|sarx|rorx|pdep|pext|mulx|bzhi)\b'
fma='\bv?f(n)?m(add|sub)(sub|add)?[0-9]{3}'
avx512='zmm|%k[0-7]\b'
# AVX2: integer ops on ymm registers (vperm2f128 and vpermilps/pd are AVX).
avx2='\bvp(add|sub|mul|and|or|xor|cmp|shuf|unpck|max|min|abs|sll|srl|sra|sign|madd|maddubs|hadd|blend|movmsk|alignr|pack|insr|extr|mov[sz]x|broadcast|gather|perm[dq]|maskmov|sllv|srlv|srav)[a-z0-9]*\b.*ymm|\bv(inserti128|extracti128|perm2i128|gather)'

count() { "$objdump" -d --no-show-raw-insn "$1" | grep -cE "$2" || true; }

executable=$(posix "$1")
shift
n=$(count "$executable" "$bmi2")
[ "$n" = 0 ] || die "$(basename "$executable") holds $n BMI2 instructions; x86_64 Hex must run without BMI2 (issue #592)."
echo "$(basename "$executable"): no BMI2."
checked=0
for lib in "$@"; do
  lib=$(posix "$lib")
  for kind in bmi2 fma avx512 avx2; do
    n=$(count "$lib" "${!kind}")
    [ "$n" = 0 ] || die "$(basename "$lib") holds $n ${kind^^} instructions; ggml must build for the x86_64 floor (scripts/ggml-x86-baseline.txt)."
  done
  echo "$(basename "$lib"): no AVX2, BMI2, FMA or AVX-512."
  checked=$((checked + 1))
done
[ $# = 0 ] || [ "$checked" -gt 0 ] || die "No ggml libraries to check."
