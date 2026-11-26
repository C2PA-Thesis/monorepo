#!/usr/bin/env bash
# Three probe samples of the reference crop proof: peak footprint, peak RSS,
# prove time and proof hash, with the proof verified. For HILLCLIMB.md.
# Usage: scripts/hillclimb-measure.sh [samples]   (after scripts/bench.sh --quick once)
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
work=$(ls -d target/bench/*/fixtures 2>/dev/null | head -1 | xargs dirname)
[ -n "$work" ] || { echo "no fixtures: run target/release/bench run --quick --samples 1" >&2; exit 1; }
for _ in $(seq "${1:-3}"); do
  /usr/bin/time -l target/release/bench --home .provenance probe --work "$work" --verify 2>&1 |
    python3 -c '
import json, re, sys
text = sys.stdin.read()
report = json.loads(next(line for line in text.splitlines() if line.startswith("{")))
rss = int(re.search(r"(\d+)\s+maximum resident set size", text).group(1))
print("footprint %7.1f MB  rss %7.1f MB  prove %6.2f s  load %5.2f s  sha %s  verified %s" % (
    report["footprint_peak"] / 1e6, rss / 1e6, report["prove_seconds"], report["load_seconds"],
    report["proof_sha256"][:16], report["verified"]))'
done
