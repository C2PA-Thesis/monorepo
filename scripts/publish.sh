#!/usr/bin/env bash
# Proves, signs and verifies a capture: the newest under captures/, or the
# id or directory given. Only signed.png in the result is meant to be shared.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$ROOT"

[ -x "$PROVENANCE" ] || die "no CLI built; run scripts/setup.sh"

if [ $# -ge 1 ]; then
  capture="$1"
  [ -d "$capture" ] || capture="captures/$1"
else
  # Capture ids start with the capture time, so the last by name is the newest.
  capture="$(find captures -mindepth 1 -maxdepth 1 -type d 2>/dev/null | sort | tail -1)"
fi
[ -d "${capture:-}" ] || die "no capture found; upload one from the phone first"

"$PROVENANCE" publish --capture "$capture"
echo
"$PROVENANCE" verify "$capture/signed.png"
