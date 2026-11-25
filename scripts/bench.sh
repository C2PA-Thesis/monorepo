#!/usr/bin/env bash
# Builds everything the benchmark needs, then runs it. Arguments go to
# `bench run`: --quick, --group NAME, --samples N, --label TEXT, --resume ID.
# Results land in crates/bench/results/<run id>/report.md.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$ROOT"

command -v cargo >/dev/null 2>&1 || die "cargo not found; install the toolchain in rust-toolchain.toml with rustup"
command -v go >/dev/null 2>&1 || die "go is not installed"

say "building the benchmark"
cargo build --release -p provenance -p bench
# Otherwise Spotlight indexes the build output during the run, on two cores.
touch target/.metadata_never_index

say "provenance setup"
"$PROVENANCE" setup

# The phone test signs with the team of the first development certificate.
if [ -z "${BENCH_IOS_TEAM:-}" ] && command -v security >/dev/null 2>&1; then
  BENCH_IOS_TEAM="$(security find-certificate -c "Apple Development" -p 2>/dev/null |
    openssl x509 -noout -subject 2>/dev/null | sed -n 's/.*OU *= *\([A-Z0-9]*\).*/\1/p')"
  export BENCH_IOS_TEAM
fi

say "running"
# A sleeping laptop would stretch whichever sample it interrupted.
keep_awake=()
command -v caffeinate >/dev/null 2>&1 && keep_awake=(caffeinate -i)
exec "${keep_awake[@]}" "$ROOT/target/release/bench" run "$@"
