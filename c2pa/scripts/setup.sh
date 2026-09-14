#!/usr/bin/env bash
# Installs c2patool and downloads the test certificates. Idempotent.
set -euo pipefail

C2PATOOL_VERSION="v0.27.16"
REPO="contentauth/c2pa-rs"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${C2PA_BIN_DIR:-$HOME/.local/bin}"

echo "==> c2patool ${C2PATOOL_VERSION}"
if command -v c2patool >/dev/null 2>&1 && c2patool --version 2>/dev/null | grep -q "${C2PATOOL_VERSION#v}"; then
  echo "    ya instalado: $(c2patool --version)"
else
  mkdir -p "$BIN_DIR"
  case "$(uname -s)" in
    Darwin) ASSET="c2patool-${C2PATOOL_VERSION}-universal-apple-darwin.zip" ;;
    Linux)  ASSET="c2patool-${C2PATOOL_VERSION}-x86_64-unknown-linux-gnu.tar.gz" ;;
    *) echo "SO no soportado por este script: $(uname -s)" >&2; exit 1 ;;
  esac
  TMP="$(mktemp -d)"
  gh release download "c2patool-${C2PATOOL_VERSION}" --repo "$REPO" --pattern "$ASSET" --clobber --dir "$TMP"
  case "$ASSET" in
    *.zip) unzip -oq "$TMP/$ASSET" -d "$TMP/x" ;;
    *.tar.gz) mkdir -p "$TMP/x" && tar xzf "$TMP/$ASSET" -C "$TMP/x" ;;
  esac
  install -m 0755 "$(find "$TMP/x" -type f -name c2patool | head -1)" "$BIN_DIR/c2patool"
  xattr -d com.apple.quarantine "$BIN_DIR/c2patool" 2>/dev/null || true
  rm -rf "$TMP"
  echo "    instalado en $BIN_DIR/c2patool"
fi

echo "==> certificados de prueba"
# SDK sample certs. Public, not a trust chain, not committed.
mkdir -p "$ROOT/certs" "$ROOT/fixtures"
for f in es256_certs.pem es256_private.key trust_anchors.pem; do
  gh api "repos/$REPO/contents/cli/sample/$f" --jq '.content' | base64 -d > "$ROOT/certs/$f"
done
chmod 600 "$ROOT/certs/es256_private.key"
gh api "repos/$REPO/contents/cli/sample/image.jpg" --jq '.content' | base64 -d > "$ROOT/fixtures/image.jpg"

echo "==> listo"
echo "    agregá $BIN_DIR al PATH si no está:  export PATH=\"$BIN_DIR:\$PATH\""
