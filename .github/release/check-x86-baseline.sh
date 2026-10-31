#!/usr/bin/env bash
# shellcheck disable=SC2034 # the patterns are read through ${!kind}
# check-x86-baseline.sh <executable> <ggml lib>...: checks that an x86_64
# release targets x86-64-v3 (Haswell and later; AVX2, BMI2, FMA, F16C) and
# nothing beyond it. Hex checks for v3 at startup (src/cpu_floor.rs), so
# its own code and everything before that check stay baseline.
#
# - ggml's static CPU libraries must use AVX2 and FMA (the build really
#   targets v3) and may hold no AVX-512, AVX-VNNI or AMX instruction.
# - The executable is only checked to be disassemblable: Rust crates that
#   detect AVX-512 at run time (blake3) use it behind that check.
#
# OBJDUMP names the disassembler (default objdump; llvm-objdump for COFF).
. "$(dirname "$0")/lib.sh"

objdump=${OBJDUMP:-objdump}
fma='\bv?f(n)?m(add|sub)(sub|add)?[0-9]{3}'
avx2='\bvp(add|sub|mul|and|or|xor|cmp|shuf|unpck|max|min|abs|sll|srl|sra|sign|madd|maddubs|hadd|blend|movmsk|alignr|pack|mov[sz]x|broadcast|gather|perm[dq]|maskmov|sllv|srlv|srav)[a-z0-9]*\b.*ymm|\bv(inserti128|extracti128|perm2i128|broadcasti128|gather|perm(ps|pd)\b)'
avx512='zmm|%k[0-7]\b'
avx_vnni='\bvpdp(busd|busds|wssd|wssds|bssd|bssds|bsud|bsuds|buud|buuds)\b'
amx='\b(tileloadd|tilestored|tdpb[a-z]+|tdpbf16ps|ldtilecfg|sttilecfg|tilerelease|tilezero)\b'

# disassemble <file>: refuses a missing file or an empty disassembly, so a
# wrong path or an unexpanded glob can never pass.
disassemble() {
  [ -f "$1" ] || die "$1 does not exist."
  "$objdump" -d --no-show-raw-insn "$1" > "$asm" || die "$objdump could not disassemble $1."
  [ "$(wc -l < "$asm")" -gt 100 ] || die "$objdump printed no code for $1."
}
count() { grep -cE "$1" "$asm" || true; }

[ $# -ge 2 ] || die "Usage: check-x86-baseline.sh <executable> <ggml lib>..."
asm=$(mktemp)
trap 'rm -f "$asm"' EXIT
disassemble "$(posix "$1")"
shift
avx2_total=0 fma_total=0
for lib in "$@"; do
  lib=$(posix "$lib")
  disassemble "$lib"
  for kind in avx512 avx_vnni amx; do
    n=$(count "${!kind}")
    [ "$n" = 0 ] || die "$(basename "$lib") holds $n ${kind^^} instructions; ggml must target x86-64-v3 and nothing beyond it."
  done
  avx2_total=$((avx2_total + $(count "$avx2")))
  fma_total=$((fma_total + $(count "$fma")))
done
if [ "$avx2_total" = 0 ] || [ "$fma_total" = 0 ]; then
  die "ggml holds no AVX2 or FMA code ($avx2_total AVX2, $fma_total FMA); the build no longer targets x86-64-v3."
fi
echo "ggml targets x86-64-v3: $avx2_total AVX2 and $fma_total FMA instructions, no AVX-512, AVX-VNNI or AMX."
