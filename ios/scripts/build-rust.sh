#!/usr/bin/env bash
# Builds the app's Rust staticlib for iPhone and the arm64 simulator and packs
# it with its headers into ios/Frameworks/ProvenanceRust.xcframework. The
# archive is crates/crop-proof-ffi, which holds the functions of
# crates/fingerprint-ffi too. Swift imports them as CropProofFFI and
# FingerprintFFI.
#
# One archive on purpose. Every Rust staticlib defines the allocator symbols,
# so with two the linker keeps one set and drops the other's
# #[global_allocator] without an error. check-allocator.sh fails on a second one.
#
# The cargo builds need only the Command Line Tools; xcodebuild
# -create-xcframework needs Xcode.
set -euo pipefail
# shellcheck source=../../scripts/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../scripts/lib.sh"
cd "$ROOT"

TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim)
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
FRAMEWORK=ProvenanceRust
CRATE=crop-proof-ffi
ARCHIVE=libcrop_proof_ffi.a
# Swift module and header of each C ABI in the archive.
MODULES=(
  "FingerprintFFI fingerprint.h"
  "CropProofFFI crop_proof.h"
)
# The app's minimum, so the linker never sees objects newer than the app.
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-17.0}"

command -v rustup >/dev/null 2>&1 || die "rustup not found; it installs the iOS targets"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

installed="$(rustup target list --installed)"
for target in "${TARGETS[@]}"; do
  grep -qx "$target" <<<"$installed" || rustup target add "$target"
  say "building for $target"
  cargo build --release --quiet -p "$CRATE" --target "$target"
done

xcodebuild -version >/dev/null 2>&1 \
  || die "built the library, but xcodebuild does not run; install Xcode and accept its license"
out="ios/Frameworks/$FRAMEWORK.xcframework"
say "assembling $out"
# Xcode copies the headers of every static xcframework into one include
# folder, so a module map at its root would collide with the others. A
# folder named after the module keeps them apart and still serves `import`.
include="$work/include"
for entry in "${MODULES[@]}"; do
  read -r module header <<<"$entry"
  mkdir -p "$include/$module"
  cp "ios/Frameworks/include/$header" "$include/$module/"
  cat > "$include/$module/module.modulemap" <<EOF
module $module {
    header "$header"
    export *
}
EOF
done
slices=()
for target in "${TARGETS[@]}"; do
  slices+=(-library "$TARGET_DIR/$target/release/$ARCHIVE" -headers "$include")
done
# Build in the temporary folder and replace the old framework only on
# success, so a failed build keeps the last good one.
xcodebuild -create-xcframework "${slices[@]}" -output "$work/$FRAMEWORK.xcframework"
rm -rf "$out"
mv "$work/$FRAMEWORK.xcframework" "$out"
echo "    $out"
