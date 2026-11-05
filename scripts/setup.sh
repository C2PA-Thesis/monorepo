#!/usr/bin/env bash
# Builds the CLI and the page, runs `provenance setup`, and checks ngrok.
# Safe to run again: every step skips what already exists.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$ROOT"

for tool in go node npm ngrok; do
  command -v "$tool" >/dev/null 2>&1 || die "$tool is not installed"
done
command -v cargo >/dev/null 2>&1 || die "cargo not found; install the toolchain in rust-toolchain.toml with rustup"

say "building the provenance CLI"
cargo build --release -p provenance

say "provenance setup"
"$PROVENANCE" setup

say "building the capture page"
rustup target list --installed | grep -q wasm32-unknown-unknown || rustup target add wasm32-unknown-unknown
(cd web && npm install --no-audit --no-fund && npm run wasm && npm run build)

say "ngrok"
if ngrok config check >/dev/null 2>&1; then
  echo "    auth token present"
else
  echo "    no auth token yet. Get one at https://dashboard.ngrok.com and run:"
  echo "    ngrok config add-authtoken <TOKEN>"
fi

say "ready. Next: scripts/serve.sh"
