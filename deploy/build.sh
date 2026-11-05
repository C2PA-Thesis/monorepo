#!/usr/bin/env bash
# Builds a release: provenance, location-proof, the pages and the scripts the
# instance runs, in one tarball. Run it on Amazon Linux 2023 so the binaries
# link against the instance's glibc.
# Usage: deploy/build.sh OUT.tgz
set -euo pipefail
[ $# -eq 1 ] || { echo "usage: $0 OUT.tgz" >&2; exit 2; }
out=$(realpath -m "$1")
cd "$(dirname "$0")/.."

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
# mktemp makes the directory 0700, and tar gives that mode to the release
# directory it extracts into, which the service user then cannot enter.
chmod 755 "$stage"
mkdir -p "$stage/bin" "$stage/deploy"

cargo build --release --locked -p provenance
install -m 755 target/release/provenance "$stage/bin/provenance"
# h3-go needs cgo, so this binary also links against glibc.
# REVISION records the commit; git refuses the container's checkout, which
# another user owns, so go cannot stamp it.
(cd location-proof && go build -trimpath -buildvcs=false -o "$stage/bin/location-proof" .)
(cd web && npm ci --no-audit --no-fund && npm run wasm && npm run build)
cp -R web/dist "$stage/web"
install -m 755 deploy/release.sh deploy/serve.sh "$stage/deploy/"
install -m 644 deploy/provenance.service "$stage/deploy/"
echo "${GITHUB_SHA:-$(git rev-parse HEAD)}" > "$stage/REVISION"

tar -czf "$out" -C "$stage" .
echo "built $out ($(du -h "$out" | cut -f1))"
