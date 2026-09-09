#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [[ "${WEB_SKIP_SETUP:-0}" != "1" ]]; then
  if [[ ! -x "$ROOT/pipeline/.venv/bin/python" || ! -f "$ROOT/pipeline/generated/hv-pst/metadata.json" || ! -f "$ROOT/pipeline/generated/zkloc-groth16/metadata.json" ]]; then
    "$ROOT/pipeline/setup.sh"
  fi
  "$ROOT/pipeline/.venv/bin/python" -m pip install --disable-pip-version-check -r "$ROOT/web/requirements.txt"
  npm --prefix "$ROOT/web" ci --no-audit --no-fund
  npm --prefix "$ROOT/web" run build
fi
export PYTHONPATH="$ROOT/pipeline:$ROOT/web${PYTHONPATH:+:$PYTHONPATH}"
cd "$ROOT"
exec "$ROOT/pipeline/.venv/bin/python" -m uvicorn server:app --host 127.0.0.1 --port "${LAB_PORT:-8042}" --timeout-graceful-shutdown 3
