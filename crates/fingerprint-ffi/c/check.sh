#!/usr/bin/env bash
# Builds the host staticlib, links c/check.c against it through
# ios/Frameworks/include/fingerprint.h and checks the test image's digest
# against the one pinned in crates/fingerprint. RUNS=n times n runs.
set -euo pipefail
# shellcheck source=../../../scripts/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../../scripts/lib.sh"
cd "$ROOT"

RUNS="${RUNS:-3}"
TARGET="${CARGO_TARGET_DIR:-$ROOT/target}"
EXPECTED="$(sed -n 's/^ *"\([0-9a-f]\{64\}\)";$/\1/p' crates/fingerprint/src/lib.rs)"
[ -n "$EXPECTED" ] || die "no pinned digest found in crates/fingerprint/src/lib.rs"

say "building the staticlib"
cargo build --release --quiet -p fingerprint-ffi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
clang -std=c11 -O2 -Wall -Wextra -Werror -I ios/Frameworks/include \
  crates/fingerprint-ffi/c/check.c "$TARGET/release/libfingerprint_ffi.a" \
  -lpthread -ldl -lm -o "$work/check"

say "fingerprinting the test image $RUNS times"
"$work/check" "$RUNS" > "$work/fingerprint.bin"
if command -v sha256sum >/dev/null 2>&1; then
  digest="$(sha256sum "$work/fingerprint.bin" | cut -d' ' -f1)"
else
  digest="$(shasum -a 256 "$work/fingerprint.bin" | cut -d' ' -f1)"
fi
[ "$digest" = "$EXPECTED" ] || die "digest $digest, expected $EXPECTED"
echo "    digest $digest matches"
