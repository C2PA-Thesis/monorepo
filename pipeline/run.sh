#!/usr/bin/env bash
set -euo pipefail

PIPELINE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "${PYTHON_BIN:-python3}" "$PIPELINE/demo.py" "$@"
