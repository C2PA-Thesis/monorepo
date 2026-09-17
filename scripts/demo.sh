#!/usr/bin/env bash
# The laptop-only demo: a simulated capture published and verified, then
# every attack, each rejected by the expected check.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$ROOT"

[ -x "$PROVENANCE" ] || die "no CLI built; run scripts/setup.sh"
"$PROVENANCE" demo "$@"
echo
"$PROVENANCE" attack
