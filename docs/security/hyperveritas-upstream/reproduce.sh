#!/usr/bin/env bash
# Runs attacks A2 and A3 of the security analysis against upstream HyperVerITAS
# (glgreiner/HyperVerITAS). Exits non-zero if any case does not end as expected.
#
# Usage: reproduce.sh [work-dir]
# The work dir must be outside the monorepo, or cargo takes upstream's crates
# for members of our workspace.
# Needs git, python3, a Rust nightly toolchain (upstream needs one; override with
# HV_TOOLCHAIN) and about 7 GB of free memory.
set -euo pipefail

rev=915c48a29252067121832ea9c51fdaff5ec9a961
here=$(cd "$(dirname "$0")" && pwd)
work=${1:-${TMPDIR:-/tmp}/hyperveritas-upstream}
mkdir -p "$work" && work=$(cd "$work" && pwd)
toolchain=${HV_TOOLCHAIN:-nightly}
src=$work/HyperVerITAS
impl=$src/hyperveritas_impl
run=$work/run

if [ ! -d "$src/.git" ]; then
  git clone --quiet https://github.com/glgreiner/HyperVerITAS "$src"
fi
git -C "$src" checkout --quiet --force "$rev"
rm -f "$impl/examples/hv_crop_pst_a3.rs"
git -C "$src" submodule update --init --quiet
git -C "$src" apply "$here/verifier-io.patch"
python3 "$here/make_a3.py" "$impl/examples/hv_crop_pst.rs" "$impl/examples/hv_crop_pst_a3.rs"

rm -rf "$run" && mkdir -p "$run" "$impl/images"
python3 "$here/fixtures.py" "$impl/images" "$run"

echo "toolchain: $(rustc "+$toolchain" --version)"
cargo "+$toolchain" build --quiet --release --manifest-path "$impl/Cargo.toml" \
  --example hv_crop_pst --example hv_crop_pst_a3
cargo build --quiet --release --manifest-path "$here/forge/Cargo.toml"
forge=$here/forge/target/release/hv-upstream-forge

failed=0
# case NAME EXPECT EXAMPLE [VAR=value...]: runs one prove-and-verify and checks
# that the verifier accepts or rejects. The example panics on rejection.
case_() {
  local name=$1 expect=$2 example=$3 got
  shift 3
  if (cd "$impl" && env "$@" "./target/release/examples/$example" 19) > "$run/$name.log" 2>&1 \
    && grep -q 'Verifier passed!: true' "$run/$name.log"; then
    got=accept
  else
    got=reject
  fi
  if [ "$got" = "$expect" ]; then
    printf '%-12s %-7s ok\n' "$name" "$got"
  else
    printf '%-12s %-7s FAILED, expected %s (see %s)\n' "$name" "$got" "$expect" "$run/$name.log"
    failed=1
  fi
}

case_ honest accept hv_crop_pst HV_DUMP_CHALLENGES="$run/challenges.txt"

"$forge" "$run/challenges.txt" "$impl/images/Crop19.json" "$run/target.json" "$run/forged.json" \
  2> "$run/forge.log"
cat "$run/forge.log"

# A2: the honest proof, unchanged, with another published image.
case_ a2 accept hv_crop_pst \
  HV_VERIFIER_CROP="$run/forged.json" HV_DUMP_CHALLENGES="$run/challenges-forged.txt"
if cmp -s "$run/challenges.txt" "$run/challenges-forged.txt"; then
  echo "a2: the crop challenges are identical for both published images"
else
  echo "a2: FAILED, the crop challenges changed with the published image"
  failed=1
fi
case_ a2-control reject hv_crop_pst HV_VERIFIER_CROP="$run/target.json"

# A3: a forged proof from the fingerprint alone, for the gradient.
case_ a3 accept hv_crop_pst_a3 HV_PROVER_CROP="$run/target.json" HV_VERIFIER_CROP="$run/target.json"
grep 'outside 0..255' "$run/a3.log"
case_ a3-control reject hv_crop_pst_a3 HV_PROVER_CROP="$run/target.json"

exit "$failed"
