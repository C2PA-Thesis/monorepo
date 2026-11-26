#!/usr/bin/env bash
# Fails unless exactly one of the static libraries the app links defines the
# Rust allocator, in each slice. Run it after build-go.sh and build-rust.sh.
#
# Every Rust staticlib defines the allocator symbols (__rust_alloc and the
# rest). With two of them the linker keeps one set, and the
# #[global_allocator] of the other library is dropped without an error. That
# is how the app once ran the crop prover on the system allocator, with
# Fingerprint.xcframework and CropProof.xcframework both linked.
#
# It reads the build files of the Frameworks phases in the Xcode project: the
# xcframeworks and the plain static libraries. It stops at an entry it cannot
# read, such as a package product, so none passes unseen. A library that only
# OTHER_LDFLAGS names is not read.
set -euo pipefail
# shellcheck source=../../scripts/lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/../../scripts/lib.sh"
cd "$ROOT"

PROJECT=ios/ProvenanceCapture.xcodeproj/project.pbxproj
# Matches the plain name and the mangled one of newer toolchains
# (..._7___rustc12___rust_alloc), and not __rust_alloc_zeroed.
SYMBOL=' T _.*__rust_alloc$'

# One line for each build file of a Frameworks phase: the id of its file
# reference and its name. A package product has no file reference and gets "-".
linked="$(sed -n \
  -e 's|.*/\* \(.*\) in Frameworks \*/ = {isa = PBXBuildFile; fileRef = \([0-9A-F]*\) .*|\2 \1|p' \
  -e 's|.*/\* \(.*\) in Frameworks \*/ = {isa = PBXBuildFile; productRef = .*|- \1|p' "$PROJECT")"
[ -n "$linked" ] || die "found no library in the Frameworks phases of $PROJECT"

# Prints where the file of a reference is. The project keeps its libraries in
# ios/Frameworks; a path from the project's folder is tried too.
locate() {
  local ref="$1" name="$2" path candidate
  path="$(sed -n "s|^[[:space:]]*$ref /\* .*isa = PBXFileReference;.* path = \"\{0,1\}\([^\";]*\)\"\{0,1\};.*|\1|p" "$PROJECT")"
  [ -n "$path" ] || die "found no path for $name in $PROJECT"
  for candidate in "ios/Frameworks/$path" "ios/$path"; do
    if [ -e "$candidate" ]; then
      echo "$candidate"
      return
    fi
  done
  die "ios/Frameworks/$path is not built; run ios/scripts/build-go.sh and build-rust.sh"
}

# How many times an archive defines the symbol. grep -c reads all of nm's
# output: grep -q would stop at the first match and nm would fail on the
# closed pipe.
definitions() { nm "$1" 2>/dev/null | grep -c "$SYMBOL" || true; }

# One line for each archive: its slice, its count of definitions, its path.
# A plain static library is linked in every build, so its slice is "all".
archives=""
while read -r ref name; do
  case "$name" in
    *.xcframework)
      dir="$(locate "$ref" "$name")"
      for archive in "$dir"/*/*.a; do
        [ -f "$archive" ] || die "$dir holds no static library"
        archives+="$(basename "$(dirname "$archive")") $(definitions "$archive") $archive"$'\n'
      done
      ;;
    *.a)
      archive="$(locate "$ref" "$name")"
      archives+="all $(definitions "$archive") $archive"$'\n'
      ;;
    # Dynamic: Rust inside one keeps its own allocator.
    *.framework | *.dylib | *.tbd) ;;
    *) die "cannot tell whether $name holds Rust: this check reads xcframeworks and static libraries only" ;;
  esac
done <<<"$linked"

slices="$(awk 'NF && $1 != "all" { print $1 }' <<<"$archives" | sort -u)"
[ -n "$slices" ] || die "found no xcframework in the Frameworks phases of $PROJECT"

say "Rust allocators in the libraries the app links"
failed=0
while read -r slice; do
  with="$(awk -v slice="$slice" '($1 == slice || $1 == "all") && $2 > 0 { print $3 }' <<<"$archives")"
  if [ -z "$with" ]; then
    # None at all means this check no longer sees the symbol, not that all is well.
    echo "    $slice: no library defines __rust_alloc" >&2
    failed=1
  elif [ "$(wc -l <<<"$with")" -gt 1 ]; then
    echo "    $slice: more than one library defines __rust_alloc:" >&2
    while read -r archive; do
      echo "      $archive" >&2
    done <<<"$with"
    failed=1
  else
    echo "    $slice: $with"
  fi
done <<<"$slices"
[ "$failed" -eq 0 ] || die "the app must link exactly one Rust static library; see crates/crop-proof/src/alloc.rs"
