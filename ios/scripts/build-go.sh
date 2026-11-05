#!/usr/bin/env bash
# Builds location-proof/mobile as a Go c-archive for iOS devices and the arm64
# Simulator and wraps both in ios/Frameworks/ZKLoc.xcframework, which Swift
# imports as module ZKLocFFI. Needs Xcode (xcrun, xcodebuild) and Go.
set -euo pipefail
# shellcheck source-path=SCRIPTDIR source=../../scripts/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../scripts/lib.sh"
cd "$ROOT"

MIN_IOS="${IOS_DEPLOYMENT_TARGET:-17.0}"
OUTPUT="$ROOT/ios/Frameworks/ZKLoc.xcframework"

command -v go >/dev/null 2>&1 || die "go is not on PATH"
command -v xcodebuild >/dev/null 2>&1 || die "xcodebuild is not on PATH; install Xcode"
xcrun --sdk iphoneos --show-sdk-path >/dev/null 2>&1 ||
  die "xcrun finds no iphoneos SDK; install Xcode and run xcode-select -s /Applications/Xcode.app"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Headers sit in a folder named after the module: Xcode copies every
# XCFramework's headers into one include directory, where two top-level
# module.modulemap files (this one and Fingerprint's) would collide.
headers="$work/headers"
mkdir -p "$headers/ZKLocFFI"
cp ios/Frameworks/include/zkloc.h "$headers/ZKLocFFI/"
cat > "$headers/ZKLocFFI/module.modulemap" <<'EOF'
module ZKLocFFI {
    header "zkloc.h"
    // The Go runtime calls CoreFoundation on iOS (runtime/cgo init_working_dir).
    link framework "CoreFoundation"
    export *
}
EOF

# The -target triple is what tells a device build from a Simulator build:
# both are GOOS=ios GOARCH=arm64 to Go.
build() {
  local sdk="$1" target="$2" sysroot cc flags
  sysroot="$(xcrun --sdk "$sdk" --show-sdk-path)"
  cc="$(xcrun --sdk "$sdk" --find clang)"
  flags="-isysroot $sysroot -target $target"
  say "building libzkloc.a for $sdk ($target)"
  mkdir -p "$work/$sdk"
  GOOS=ios GOARCH=arm64 CGO_ENABLED=1 CC="$cc" CGO_CFLAGS="$flags" CGO_LDFLAGS="$flags" \
    go -C location-proof build -trimpath -buildmode=c-archive \
    -o "$work/$sdk/libzkloc.a" ./mobile
  # Go writes its own header next to the archive; zkloc.h replaces it.
  rm -f "$work/$sdk/libzkloc.h"
}

build iphoneos "arm64-apple-ios$MIN_IOS"
build iphonesimulator "arm64-apple-ios$MIN_IOS-simulator"

say "assembling $OUTPUT"
rm -rf "$OUTPUT"
xcodebuild -create-xcframework \
  -library "$work/iphoneos/libzkloc.a" -headers "$headers" \
  -library "$work/iphonesimulator/libzkloc.a" -headers "$headers" \
  -output "$OUTPUT"

for sdk in iphoneos iphonesimulator; do
  echo "    $sdk: $(du -h "$work/$sdk/libzkloc.a" | cut -f1)"
done
