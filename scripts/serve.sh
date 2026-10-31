#!/usr/bin/env bash
# Serves the capture page, the presenter page and the API, opens an ngrok
# tunnel for the capture page in the phone's browser, listens with TLS on
# the local network for the app, and opens the presenter page on this
# laptop. Each upload is proved, signed and verified as soon as it arrives.
# Ctrl-C stops.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$ROOT"

PORT="${PORT:-8791}"
TLS_PORT="${TLS_PORT:-8792}"
HOME_DIR="${PROVENANCE_HOME:-.provenance}"
[ -x "$PROVENANCE" ] || die "no CLI built; run scripts/setup.sh"
[ -f web/dist/index.html ] || die "no page built; run scripts/setup.sh"
[ -f "$HOME_DIR/tls/key.pem" ] || die "no TLS key for the app; run scripts/setup.sh"
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
echo "    app: scan the large QR code in Provenance Capture; it connects over TLS to"
# The same addresses serve puts in the QR code: not loopback, not link-local.
lan_ips="$(ifconfig 2>/dev/null | awk '$1 == "inet" && $2 !~ /^(127\.|169\.254\.)/ { print $2 }' | sort -u)"
if [ -n "$lan_ips" ]; then
  for ip in $lan_ips; do echo "      https://$ip:$TLS_PORT"; done
else
  echo "      port $TLS_PORT, once this laptop joins a network (then restart this script)"
fi
echo "    Simulator: the app reads the pairing payload from http://127.0.0.1:$PORT/api/v2/pairing"
echo "    browser: scan the small one with the phone's camera; the phone may show an ngrok notice first"
echo "    macOS may ask whether provenance can accept incoming connections: allow it for the app"
( sleep 1.5; open_url "$screen" ) &

# A child rather than exec, so the trap above still stops ngrok on Ctrl-C.
# serve prints the app's https addresses and the key pin under "app" and "pin".
"$PROVENANCE" serve --port "$PORT" --tls-port "$TLS_PORT" --web web/dist --public-url "$url"
