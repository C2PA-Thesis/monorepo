#!/usr/bin/env bash
# Runs provenance serve for systemd. The public IP sits behind NAT, so it is
# asked of the metadata service and announced with --app-url; without one
# (not on EC2) the server announces only its interfaces.
set -euo pipefail
release=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
data=${PROVENANCE_DATA:-/var/lib/provenance}
imds=http://169.254.169.254/latest
args=()
if token=$(curl -sf --max-time 2 -X PUT "$imds/api/token" -H 'X-aws-ec2-metadata-token-ttl-seconds: 60') &&
  ip=$(curl -sf --max-time 2 -H "X-aws-ec2-metadata-token: $token" "$imds/meta-data/public-ipv4"); then
  args+=(--app-url "https://$ip:8792")
fi
exec "$release/bin/provenance" --home "$data/home" serve \
  --captures "$data/captures" --web "$release/web" "${args[@]}"
