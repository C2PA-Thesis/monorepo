#!/usr/bin/env bash
# Serves the capture page and API, opens an ngrok tunnel to it, and prints
# the URL for the phone. Ctrl-C stops both.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$ROOT"

PORT="${PORT:-8791}"
[ -x "$PROVENANCE" ] || die "no CLI built; run scripts/setup.sh"
[ -f web/dist/index.html ] || die "no page built; run scripts/setup.sh"
ngrok config check >/dev/null 2>&1 || die "ngrok has no auth token; run: ngrok config add-authtoken <TOKEN>"

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

say "open this on the phone: $url"
echo "    (click through the ngrok notice; pair once with the code below)"
# A child rather than exec, so the trap above still stops ngrok on Ctrl-C.
"$PROVENANCE" serve --port "$PORT" --web web/dist
