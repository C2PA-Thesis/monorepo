#!/usr/bin/env bash
# Serves the capture page, the presenter page and the API, opens an ngrok
# tunnel for the phone, and opens the presenter page on this laptop. Each
# upload is proved, signed and verified as soon as it arrives. Ctrl-C stops.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$ROOT"

PORT="${PORT:-8791}"
[ -x "$PROVENANCE" ] || die "no CLI built; run scripts/setup.sh"
[ -f web/dist/index.html ] || die "no page built; run scripts/setup.sh"
ngrok config check >/dev/null 2>&1 || die "ngrok has no auth token; run: ngrok config add-authtoken <TOKEN>"

# Rebuild what changed since the last run, so the demo never serves stale code.
say "checking the build"
cargo build --release --quiet -p provenance
if [ -n "$(find web/src web/index.html web/screen.html -newer web/dist/index.html 2>/dev/null | head -1)" ]; then
  (cd web && npm run --silent build >/dev/null)
  echo "    rebuilt the pages"
fi

# Leftovers from an earlier run would take the port and the ngrok session.
pkill -f "provenance serve" 2>/dev/null || true
pkill -f "ngrok http" 2>/dev/null || true

ngrok http "$PORT" --log stdout --log-format json > /dev/null 2>&1 &
NGROK=$!
trap 'kill $NGROK 2>/dev/null || true' EXIT

url=""
for _ in $(seq 1 30); do
  sleep 0.5
  url="$(curl -sS http://127.0.0.1:4040/api/tunnels 2>/dev/null | sed -n 's/.*"public_url":"\(https:[^"]*\)".*/\1/p' | head -1)"
  [ -n "$url" ] && break
done
[ -n "$url" ] || die "ngrok did not open a tunnel"

screen="http://127.0.0.1:$PORT/screen"
say "presenter page: $screen"
echo "    scan its QR code with the phone; the phone may show an ngrok notice first"
( sleep 1.5; open_url "$screen" ) &

# A child rather than exec, so the trap above still stops ngrok on Ctrl-C.
"$PROVENANCE" serve --port "$PORT" --web web/dist --public-url "$url"
