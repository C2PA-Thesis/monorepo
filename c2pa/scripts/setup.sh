#!/usr/bin/env bash
set -euo pipefail

C2PATOOL_VERSION="v0.27.16"
REPO="contentauth/c2pa-rs"
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="${C2PA_BIN_DIR:-$HOME/.local/bin}"

echo "==> c2patool ${C2PATOOL_VERSION}"
if command -v c2patool >/dev/null 2>&1 && c2patool --version 2>/dev/null | grep -q "${C2PATOOL_VERSION#v}"; then
  echo "    already installed: $(c2patool --version)"
else
  echo "==> Download c2patool ${C2PATOOL_VERSION} from GitHub repository $REPO"
  mkdir -p "$BIN_DIR"
  case "$(uname -s)" in
    Darwin) ASSET="c2patool-${C2PATOOL_VERSION}-universal-apple-darwin.zip" ;;
    Linux)  ASSET="c2patool-${C2PATOOL_VERSION}-x86_64-unknown-linux-gnu.tar.gz" ;;
    *) echo "unsupported operating system: $(uname -s)" >&2; exit 1 ;;
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
  echo "    installed at $BIN_DIR/c2patool"
fi

echo "==> C2PA test certificates"
# SDK sample certs. Public, not a trust chain, not committed.
mkdir -p "$ROOT/certs" "$ROOT/fixtures"
for f in es256_certs.pem es256_private.key trust_anchors.pem; do
  echo "==> Download the public SDK test file cli/sample/$f from $REPO to $ROOT/certs/$f"
  gh api "repos/$REPO/contents/cli/sample/$f" --jq '.content' | base64 -d > "$ROOT/certs/$f"
done
chmod 600 "$ROOT/certs/es256_private.key"
echo "==> Download the SDK sample photo cli/sample/image.jpg from $REPO to $ROOT/fixtures/image.jpg"
gh api "repos/$REPO/contents/cli/sample/image.jpg" --jq '.content' | base64 -d > "$ROOT/fixtures/image.jpg"

echo "==> setup complete"
echo "    add $BIN_DIR to PATH if needed: export PATH=\"$BIN_DIR:\$PATH\""
