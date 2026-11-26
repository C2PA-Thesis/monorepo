#!/usr/bin/env bash
# Probe samples of the reference crop proof: peak footprint, peak RSS, prove
# time and proof hash, with the proof verified. For crates/crop-proof/HILLCLIMB.md.
# Usage: scripts/hillclimb-measure.sh [SAMPLES] [WORK]   (after scripts/bench.sh --quick)
# SAMPLES defaults to 3. WORK is a benchmark run's directory under
# target/bench, by default the first one that has fixtures.
set -euo pipefail
# shellcheck source=lib.sh
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"
cd "$ROOT"

BENCH="$ROOT/target/release/bench"
HOME_DIR="${PROVENANCE_HOME:-.provenance}"
[ -x "$BENCH" ] || die "no benchmark built; run scripts/bench.sh --quick"

work="${2:-}"
if [ -z "$work" ]; then
  for fixtures in target/bench/*/fixtures; do
    [ -d "$fixtures" ] || continue
    work="${fixtures%/fixtures}"
    break
  done
fi
[ -d "$work/fixtures" ] || die "no fixtures; run scripts/bench.sh --quick"

# The footprint is Apple's; `time -l` and the probe report it only there.
for _ in $(seq "${1:-3}"); do
  /usr/bin/time -l "$BENCH" --home "$HOME_DIR" probe --work "$work" --verify 2>&1 |
    python3 -c '
import json, re, sys
text = sys.stdin.read()
line = next((line for line in text.splitlines() if line.startswith("{")), None)
if line is None:
    sys.exit("error: the probe printed no report:\n" + text)
report = json.loads(line)
if "footprint_peak" not in report:
    sys.exit("error: the probe measured no footprint on this system")
rss = int(re.search(r"(\d+)\s+maximum resident set size", text).group(1))
print("footprint %7.1f MB  rss %7.1f MB  prove %6.2f s  load %5.2f s  sha %s  verified %s" % (
    report["footprint_peak"] / 1e6, rss / 1e6, report["prove_seconds"], report["load_seconds"],
    report["proof_sha256"][:16], report["verified"]))'
done
