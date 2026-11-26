#!/usr/bin/env bash
# Builds the Rust staticlibs for iPhone and the arm64 simulator and packs each
# with its header into an XCFramework in ios/Frameworks:
#   Fingerprint.xcframework  crates/fingerprint-ffi  import FingerprintFFI
#   CropProof.xcframework    crates/crop-proof-ffi   import CropProofFFI
# The cargo builds need only the Command Line Tools; xcodebuild
# -create-xcframework needs Xcode.
set -euo pipefail
# shellcheck source=../../scripts/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../scripts/lib.sh"
cd "$ROOT"

TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim)
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
# Framework, crate, library, header and Swift module of each staticlib.
LIBRARIES=(
  "Fingerprint fingerprint-ffi libfingerprint_ffi.a fingerprint.h FingerprintFFI"
  "CropProof crop-proof-ffi libcrop_proof_ffi.a crop_proof.h CropProofFFI"
)
# The app's minimum, so the linker never sees objects newer than the app.
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-17.0}"

command -v rustup >/dev/null 2>&1 || die "rustup not found; it installs the iOS targets"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

packages=()
for library in "${LIBRARIES[@]}"; do
  read -r _ crate _ <<<"$library"
  packages+=(-p "$crate")
done

installed="$(rustup target list --installed)"
for target in "${TARGETS[@]}"; do
  grep -qx "$target" <<<"$installed" || rustup target add "$target"
  say "building for $target"
  # One build for all the crates: the app links every staticlib, and the
  # crates they share must be the same objects in each, or the linker would
  # see two different copies of one symbol.
  cargo build --release --quiet "${packages[@]}" --target "$target"
done

xcodebuild -version >/dev/null 2>&1 \
  || die "built the libraries, but xcodebuild does not run; install Xcode and accept its license"
for library in "${LIBRARIES[@]}"; do
  read -r framework _ archive header module <<<"$library"
  out="ios/Frameworks/$framework.xcframework"
  say "assembling $out"
  # Xcode copies the headers of every static xcframework into one include
  # folder, so a module map at its root would collide with the others. A
  # folder named after the module keeps them apart and still serves `import`.
  include="$work/$framework/include"
  mkdir -p "$include/$module"
  cp "ios/Frameworks/include/$header" "$include/$module/"
  cat > "$include/$module/module.modulemap" <<EOF
module $module {
    header "$header"
    export *
}
EOF
  slices=()
  for target in "${TARGETS[@]}"; do
    slices+=(-library "$TARGET_DIR/$target/release/$archive" -headers "$include")
  done
  # Build in the temporary folder and replace the old framework only on
  # success, so a failed build keeps the last good one.
  xcodebuild -create-xcframework "${slices[@]}" -output "$work/$framework.xcframework"
  rm -rf "$out"
  mv "$work/$framework.xcframework" "$out"
  echo "    $out"
done
