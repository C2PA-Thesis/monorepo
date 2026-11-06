#!/usr/bin/env bash
# Runs provenance serve for systemd, with registration open to the app. The
# public IP sits behind NAT, so it is asked of the metadata service and
# announced with --app-url; without one (not on EC2) the server announces only
# its interfaces. On EC2 the Team ID and the DeviceCheck key for Apple's risk
# metric come from SSM under /provenance/apple; without the key the metric is
# not checked, and the log says so.
set -euo pipefail
release=$(cd "$(dirname "$(readlink -f "$0")")/.." && pwd)
data=${PROVENANCE_DATA:-/var/lib/provenance}
bundle=io.github.c2pa-thesis.capture
# Not yet confirmed as the paid team's ID; SSM's team-id wins when present.
team=678K8MRCKG
imds=http://169.254.169.254/latest
args=(--open-registration)
if token=$(curl -sf --max-time 2 -X PUT "$imds/api/token" -H 'X-aws-ec2-metadata-token-ttl-seconds: 60'); then
  meta() { curl -sf --max-time 2 -H "X-aws-ec2-metadata-token: $token" "$imds/meta-data/$1"; }
  if ip=$(meta public-ipv4); then
    args+=(--app-url "https://$ip:8792")
  fi
  region=$(meta placement/region) || region=""
  param() {
    aws ssm get-parameter --region "$region" --name "/provenance/apple/$1" \
      --with-decryption --query Parameter.Value --output text
  }
  if id=$(param team-id); then
    team=$id
  fi
  if key=$(param app-attest-key) && key_id=$(param app-attest-key-id); then
    export PROVENANCE_DEVICECHECK_KEY=$key
    args+=(--devicecheck-key-id "$key_id")
  else
    echo "no DeviceCheck key in SSM under /provenance/apple; the risk metric is not checked" >&2
  fi
fi
export PROVENANCE_APP_ID=${PROVENANCE_APP_ID:-$team.$bundle}
exec "$release/bin/provenance" --home "$data/home" serve \
  --captures "$data/captures" --web "$release/web" "${args[@]}"
