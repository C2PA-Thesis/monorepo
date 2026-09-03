#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PIPELINE="$ROOT/pipeline"
GENERATED="$PIPELINE/generated"
VENV="$PIPELINE/.venv"
PYTHON_BIN="${PYTHON_BIN:-python3}"

if ! command -v cargo >/dev/null 2>&1; then
  for candidate in "${HOME}"/.rustup/toolchains/nightly-*/bin; do
    if [[ -x "$candidate/cargo" ]]; then
      export PATH="$candidate:$PATH"
      break
    fi
  done
fi

for command_name in git go cargo rustc "$PYTHON_BIN"; do
  if ! command -v "$command_name" >/dev/null 2>&1; then
    echo "missing required tool: $command_name" >&2
    exit 1
  fi
done

echo "==> Python dependencies"
if [[ ! -x "$VENV/bin/python" ]]; then
  "$PYTHON_BIN" -m venv "$VENV"
fi
"$VENV/bin/python" -m pip install --disable-pip-version-check -r "$PIPELINE/requirements.txt"

echo "==> c2patool and C2PA test signing material"
"$ROOT/c2pa/scripts/setup.sh"
export PATH="${C2PA_BIN_DIR:-${HOME}/.local/bin}:$PATH"

if [[ ! -f "$ROOT/.gitmodules" ]]; then
  echo "editproof and locproof submodules have not been added yet" >&2
  exit 1
fi

echo "==> proof submodules"
git -C "$ROOT" submodule update --init --recursive -- editproof locproof

echo "==> HyperVerITAS release CLI"
cargo build \
  --release \
  --manifest-path "$ROOT/editproof/hyperveritas_impl/Cargo.toml" \
  --bin hv
HV_BIN="${HV_BIN:-$ROOT/editproof/hyperveritas_impl/target/release/hv}"

echo "==> ZKLP release CLI"
mkdir -p "$GENERATED/bin"
(
  cd "$ROOT/locproof"
  go build -o "$GENERATED/bin/zkloc" ./cmd/zkloc
)
ZKLOC_BIN="${ZKLOC_BIN:-$GENERATED/bin/zkloc}"

mkdir -p "$ROOT/fixtures/generated"

echo "==> cached proof parameters"
if [[ ! -f "$GENERATED/hv-pst/metadata.json" ]]; then
  "$HV_BIN" setup-pst --size 19 --out "$GENERATED/hv-pst"
fi
if [[ ! -f "$GENERATED/zkloc-groth16/metadata.json" ]]; then
  "$ZKLOC_BIN" setup --out "$GENERATED/zkloc-groth16"
fi

echo "==> separate demo device identity"
DEVICE_PRIVATE="$ROOT/fixtures/generated/device-private.pem"
DEVICE_PUBLIC="$ROOT/fixtures/generated/device-public.pem"
if [[ ! -f "$DEVICE_PRIVATE" || ! -f "$DEVICE_PUBLIC" ]]; then
  "$VENV/bin/python" "$PIPELINE/generate_device_key.py" \
    --private "$DEVICE_PRIVATE" \
    --public "$DEVICE_PUBLIC"
fi
cp "$ROOT/c2pa/fixtures/image.jpg" "$ROOT/fixtures/generated/demo-input.jpg"

echo "==> exact tool versions"
"$VENV/bin/python" "$PIPELINE/record_versions.py" \
  --root "$ROOT" \
  --hv "$HV_BIN" \
  --zkloc "$ZKLOC_BIN" \
  --out "$GENERATED/tool-versions.json"

echo "setup complete"
