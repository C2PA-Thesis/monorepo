#!/usr/bin/env bash
# Builds the host staticlib, links c/check.c against it through
# ios/Frameworks/include/crop_proof.h and fingerprint.h, checks that its
# allocator is the one in use, and makes and checks one crop proof at the
# reference case. It proves twice (the first call learns the proof's
# length), so it takes as long and as much memory as two crop proofs. The
# parameters come from `provenance setup`; CROP_PARAMS names another directory.
set -euo pipefail
# shellcheck source=../../../scripts/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib.sh"
cd "$ROOT"

TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
PARAMS="${CROP_PARAMS:-$ROOT/.provenance/params/crop}"
[ -f "$PARAMS/prover.bin" ] \
  || die "no crop parameters in $PARAMS; run 'provenance setup' or set CROP_PARAMS"

say "building the staticlib"
cargo build --release --quiet -p crop-proof-ffi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
clang -std=c11 -O2 -Wall -Wextra -Werror -I ios/Frameworks/include \
  crates/crop-proof-ffi/c/check.c "$TARGET/release/libcrop_proof_ffi.a" \
  -lpthread -ldl -lm -o "$work/check"

say "proving and verifying the reference crop"
"$work/check" "$PARAMS" > "$work/proof.bin"
if command -v sha256sum >/dev/null 2>&1; then
  digest="$(sha256sum "$work/proof.bin" | cut -d' ' -f1)"
else
  digest="$(shasum -a 256 "$work/proof.bin" | cut -d' ' -f1)"
fi
echo "    proof of $(wc -c < "$work/proof.bin" | tr -d ' ') bytes, SHA-256 $digest"
