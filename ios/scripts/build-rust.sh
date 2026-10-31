#!/usr/bin/env bash
# Builds the fingerprint staticlib (crates/fingerprint-ffi) for iPhone and the
# arm64 simulator and packs it with its header into
# ios/Frameworks/Fingerprint.xcframework. The cargo builds need only the
# Command Line Tools; xcodebuild -create-xcframework needs Xcode.
set -euo pipefail
# shellcheck source=../../scripts/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../scripts/lib.sh"
cd "$ROOT"

TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim)
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
OUT=ios/Frameworks/Fingerprint.xcframework
# The app's minimum, so the linker never sees objects newer than the app.
export IPHONEOS_DEPLOYMENT_TARGET="${IPHONEOS_DEPLOYMENT_TARGET:-17.0}"

command -v rustup >/dev/null 2>&1 || die "rustup not found; it installs the iOS targets"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
# Xcode copies the headers of every static xcframework into one include
# folder, so a module map at its root would collide with ZKLoc's. A folder
# named after the module keeps them apart and still serves `import FingerprintFFI`.
mkdir -p "$work/include/FingerprintFFI"
cp ios/Frameworks/include/fingerprint.h "$work/include/FingerprintFFI/"
cat > "$work/include/FingerprintFFI/module.modulemap" <<'EOF'
module FingerprintFFI {
    header "fingerprint.h"
    export *
}
EOF

installed="$(rustup target list --installed)"
libraries=()
for target in "${TARGETS[@]}"; do
  grep -qx "$target" <<<"$installed" || rustup target add "$target"
  say "building for $target"
  cargo build --release --quiet -p fingerprint-ffi --target "$target"
  libraries+=(-library "$TARGET_DIR/$target/release/libfingerprint_ffi.a" -headers "$work/include")
done

xcodebuild -version >/dev/null 2>&1 \
  || die "built the libraries, but xcodebuild does not run; install Xcode and accept its license"
say "assembling $OUT"
# Build in the temporary folder and replace the old framework only on success,
# so a failed build keeps the last good one.
xcodebuild -create-xcframework "${libraries[@]}" -output "$work/Fingerprint.xcframework"
rm -rf "$OUT"
mv "$work/Fingerprint.xcframework" "$OUT"
echo "    $OUT"
