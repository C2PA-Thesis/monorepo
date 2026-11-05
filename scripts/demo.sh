#!/usr/bin/env bash
# The demo, in one of two modes:
#
#   scripts/demo.sh phone    capture on a phone; this laptop proves, signs and verifies
#   scripts/demo.sh [ARGS]   laptop only: a simulated capture published and verified,
#                            then every attack, each rejected by the expected check.
#                            ARGS go to `provenance demo`, e.g. --out DIR.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$ROOT"

case "${1:-}" in
  phone) shift; exec "$ROOT/scripts/serve.sh" "$@" ;;
  laptop) shift ;;
esac

[ -x "$PROVENANCE" ] || die "no CLI built; run scripts/setup.sh"

# The attacks tamper with the run the demo just wrote.
out="out"
args=("$@")
for i in "${!args[@]}"; do
  [ "${args[$i]}" = "--out" ] && out="${args[$((i + 1))]:-out}"
done

"$PROVENANCE" demo "$@"
echo
"$PROVENANCE" attack --run "$out"
