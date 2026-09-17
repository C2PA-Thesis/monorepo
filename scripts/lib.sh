#!/usr/bin/env bash
# Shared by the scripts here: the repository root, the pinned Rust toolchain
# on PATH, and the provenance binary.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck disable=SC2034  # used by the scripts that source this file
PROVENANCE="$ROOT/target/release/provenance"

# rustup installed through Homebrew puts no cargo proxy on PATH, so reach the
# toolchain rust-toolchain.toml pins directly.
if ! command -v cargo >/dev/null 2>&1; then
  channel="$(sed -n 's/^channel = "\(.*\)"/\1/p' "$ROOT/rust-toolchain.toml")"
  for bin in "$HOME/.cargo/bin" "$HOME"/.rustup/toolchains/"$channel"-*/bin; do
    [ -d "$bin" ] && PATH="$bin:$PATH"
  done
  export PATH
fi

say() { printf '\n==> %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }
