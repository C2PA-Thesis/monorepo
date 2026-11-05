#!/usr/bin/env bash
# Activates the release this script ships in, as root on the instance. Data
# stays in $PROVENANCE_DATA across releases. When the new release fails its
# check, the previous one is put back and the script fails.
set -euo pipefail
release=$(cd "$(dirname "$0")/.." && pwd)
root=${PROVENANCE_ROOT:-/opt/provenance}
data=${PROVENANCE_DATA:-/var/lib/provenance}
user=provenance

host_setup() {
  id "$user" >/dev/null 2>&1 || useradd --system --home-dir "$data" --shell /sbin/nologin "$user"
  install -d -o "$user" -g "$user" "$data" "$data/home" "$data/captures"
  # The crop proof peaks at 5.7 GB on an 8 GiB instance.
  if [ "$(swapon --noheadings | wc -l)" -eq 0 ]; then
    [ -f /swapfile ] || {
      dd if=/dev/zero of=/swapfile bs=1M count=4096 status=none
      chmod 600 /swapfile
      mkswap /swapfile >/dev/null
    }
    swapon /swapfile
    grep -q '^/swapfile ' /etc/fstab || echo '/swapfile swap swap defaults 0 0' >>/etc/fstab
  fi
  install -m 644 "$release/deploy/provenance.service" /etc/systemd/system/provenance.service
  systemctl daemon-reload
  systemctl enable provenance >/dev/null 2>&1
}

as_user() {
  (cd "$data" && runuser -u "$user" -- "$@")
}

activate() {
  ln -sfn "$1" "$root/current.next"
  mv -T "$root/current.next" "$root/current"
  systemctl restart provenance
}

# The listener answers, then a whole capture is proved, published and verified.
check() {
  local code="" out
  for _ in $(seq 60); do
    code=$(curl -sk -o /dev/null -w '%{http_code}' https://127.0.0.1:8792/api/devices/none || true)
    [ "$code" = 400 ] && break
    sleep 2
  done
  if [ "$code" != 400 ]; then
    echo "the TLS listener did not answer"
    journalctl -u provenance --no-pager -n 30 || true
    return 1
  fi
  out=$(mktemp -d)
  chown "$user" "$out"
  as_user "$root/current/bin/provenance" --home "$data/home" demo --out "$out"
  rm -rf "$out"
}

previous=$(readlink "$root/current" || true)
echo "activating $(cat "$release/REVISION")"
host_setup
as_user "$release/bin/provenance" --home "$data/home" setup --location-proof "$release/bin/location-proof"
activate "$release"
if ! check; then
  echo "release $(basename "$release") failed its check"
  if [ -n "$previous" ] && [ "$previous" != "$release" ] && [ -d "$previous" ]; then
    activate "$previous"
    echo "back on $(basename "$previous")"
  fi
  exit 1
fi

# Keep the three newest releases.
find "$root/releases" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %p\n' |
  sort -rn | tail -n +4 | cut -d' ' -f2- | xargs -r rm -rf

pin=$(openssl pkey -in "$data/home/tls/key.pem" -pubout -outform der | sha256sum | cut -d' ' -f1)
echo "pin $pin"
