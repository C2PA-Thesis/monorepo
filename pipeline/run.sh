#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PIPELINE="$ROOT/pipeline"
OUT="$PIPELINE/out"
GENERATED="$PIPELINE/generated"

if [[ "${PIPELINE_SKIP_SETUP:-0}" != "1" ]]; then
  "$PIPELINE/setup.sh"
fi

PYTHON="$PIPELINE/.venv/bin/python"
HV_BIN="${HV_BIN:-$ROOT/editproof/hyperveritas_impl/target/release/hv}"
ZKLOC_BIN="${ZKLOC_BIN:-$GENERATED/bin/zkloc}"
export HV_BIN ZKLOC_BIN
export PATH="${C2PA_BIN_DIR:-${HOME}/.local/bin}:$PATH"
export C2PA_PRIVATE_KEY="$(<"$ROOT/c2pa/certs/es256_private.key")"
export C2PA_SIGN_CERT="$(<"$ROOT/c2pa/certs/es256_certs.pem")"

if [[ "$OUT" != "$ROOT/pipeline/out" ]]; then
  echo "refusing to clear an unexpected output path" >&2
  exit 1
fi
rm -rf "$OUT"
mkdir -p "$OUT/capture" "$OUT/edit" "$OUT/pi-img"
TIMINGS="$OUT/timings.tsv"
printf 'operation\twall_seconds\n' > "$TIMINGS"

run_timed() {
  local label="$1"
  shift
  local started ended elapsed
  started="$("$PYTHON" -c 'import time; print(time.time_ns())')"
  "$@"
  ended="$("$PYTHON" -c 'import time; print(time.time_ns())')"
  elapsed="$("$PYTHON" - "$started" "$ended" <<'PY'
import sys
print("{:.6f}".format((int(sys.argv[2]) - int(sys.argv[1])) / 1_000_000_000))
PY
)"
  printf '%s\t%s\n' "$label" "$elapsed" | tee -a "$TIMINGS"
}

PHOTO="${DEMO_PHOTO:-$ROOT/fixtures/generated/demo-input.jpg}"
REGION="$ROOT/fixtures/region-utdt-res7.json"
DEVICE_PRIVATE="$ROOT/fixtures/generated/device-private.pem"
DEVICE_PUBLIC="$ROOT/fixtures/generated/device-public.pem"

run_timed capture \
  "$PYTHON" "$PIPELINE/capture.py" "$PHOTO" \
    --lat -34.5478 \
    --lon -58.4462 \
    --device-key "$DEVICE_PRIVATE" \
    --out "$OUT/capture"

run_timed crop \
  "$PYTHON" "$PIPELINE/crop.py" \
    --capture "$OUT/capture" \
    --out "$OUT/edit"

run_timed location-prover \
  "$ZKLOC_BIN" prove \
    --params "$GENERATED/zkloc-groth16" \
    --secrets "$OUT/capture/secrets.json" \
    --region "$REGION" \
    --out "$OUT/pi-loc.json"

run_timed image-prover \
  "$HV_BIN" prove-crop \
    --params "$GENERATED/hv-pst" \
    --original "$OUT/capture/original.json" \
    --edited "$OUT/edit/edited.json" \
    --fingerprint "$OUT/capture/fingerprint.json" \
    --out "$OUT/pi-img"

run_timed c2pa-package \
  "$PYTHON" "$PIPELINE/package.py" \
    --receipt "$OUT/capture/receipt.json" \
    --region "$REGION" \
    --pi-loc "$OUT/pi-loc.json" \
    --pi-img "$OUT/pi-img" \
    --edited "$OUT/edit/edited.png" \
    --out "$OUT/signed.png"

run_timed verifier \
  "$PYTHON" "$PIPELINE/verify.py" "$OUT/signed.png" \
    --region "$REGION" \
    --device-public-key "$DEVICE_PUBLIC" \
    --hv-params "$GENERATED/hv-pst" \
    --zkloc-vk "$GENERATED/zkloc-groth16"

run_timed receipt-verification-benchmark \
  "$PYTHON" "$PIPELINE/benchmark_receipt.py" \
    --receipt "$OUT/capture/receipt.json" \
    --device-public-key "$DEVICE_PUBLIC" \
    --out "$OUT/receipt-verification.json"

run_timed binding-negative-integration \
  "$PYTHON" "$PIPELINE/integration_negatives.py" \
    --valid "$OUT" \
    --photo "$PHOTO" \
    --region "$REGION" \
    --device-private-key "$DEVICE_PRIVATE" \
    --device-public-key "$DEVICE_PUBLIC" \
    --hv-params "$GENERATED/hv-pst" \
    --zkloc-params "$GENERATED/zkloc-groth16" \
    --out "$OUT/negative"

"$PYTHON" "$PIPELINE/artifact_report.py" \
  --pi-img "$OUT/pi-img" \
  --pi-loc "$OUT/pi-loc.json" \
  --manifest-info "$OUT/manifest-store.txt" \
  --signed "$OUT/signed.png"

echo "wall timings"
column -t -s $'\t' "$TIMINGS" 2>/dev/null || cat "$TIMINGS"
