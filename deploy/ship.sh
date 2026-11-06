#!/usr/bin/env bash
# Ships a release from GitHub Actions to an environment's instance: uploads
# it, starts the instance when it is stopped, runs release.sh on it through
# Session Manager, and checks from outside that the app's listener answers
# with the key the instance reports.
# Usage: INSTANCE_ID=... RELEASES_BUCKET=... REVISION=... deploy/ship.sh RELEASE.tgz
set -euo pipefail
[ $# -eq 1 ] || { echo "usage: $0 RELEASE.tgz" >&2; exit 2; }
: "${INSTANCE_ID:?}" "${RELEASES_BUCKET:?}" "${REVISION:?}"
key="releases/$REVISION.tgz"

aws s3 cp "$1" "s3://$RELEASES_BUCKET/$key" --only-show-errors

state() {
  aws ec2 describe-instances --instance-ids "$INSTANCE_ID" \
    --query 'Reservations[0].Instances[0].State.Name' --output text
}
case "$(state)" in
  running) ;;
  stopping) aws ec2 wait instance-stopped --instance-ids "$INSTANCE_ID" ;&
  stopped)
    echo "starting $INSTANCE_ID"
    aws ec2 start-instances --instance-ids "$INSTANCE_ID" >/dev/null
    aws ec2 wait instance-running --instance-ids "$INSTANCE_ID"
    ;;
  pending) aws ec2 wait instance-running --instance-ids "$INSTANCE_ID" ;;
  *) echo "the instance is $(state)" >&2; exit 1 ;;
esac

ping=""
for _ in $(seq 60); do
  ping=$(aws ssm describe-instance-information --filters "Key=InstanceIds,Values=$INSTANCE_ID" \
    --query 'InstanceInformationList[0].PingStatus' --output text)
  [ "$ping" = Online ] && break
  sleep 5
done
[ "$ping" = Online ] || { echo "the instance never reached Session Manager" >&2; exit 1; }

dir="/opt/provenance/releases/$REVISION"
parameters=$(jq -n --arg url "s3://$RELEASES_BUCKET/$key" --arg dir "$dir" '{
  executionTimeout: ["3600"],
  commands: [
    "set -e",
    "rm -rf \($dir) && mkdir -p \($dir)",
    "aws s3 cp --only-show-errors \($url) /tmp/release.tgz",
    "tar -xzf /tmp/release.tgz -C \($dir) && rm /tmp/release.tgz",
    "\($dir)/deploy/release.sh"
  ]}')
command=$(aws ssm send-command --instance-ids "$INSTANCE_ID" --document-name AWS-RunShellScript \
  --comment "release $REVISION" --parameters "$parameters" --query Command.CommandId --output text)

status=Pending
while :; do
  sleep 10
  status=$(aws ssm get-command-invocation --command-id "$command" --instance-id "$INSTANCE_ID" \
    --query Status --output text 2>/dev/null || echo Pending)
  case "$status" in Pending | InProgress | Delayed) ;; *) break ;; esac
done
output=$(aws ssm get-command-invocation --command-id "$command" --instance-id "$INSTANCE_ID" \
  --query '[StandardOutputContent, StandardErrorContent]' --output text)
echo "$output"
[ "$status" = Success ] || { echo "release.sh ended $status" >&2; exit 1; }

expected=$(grep -oE '^pin [0-9a-f]{64}' <<<"$output" | cut -d' ' -f2)
ip=$(aws ec2 describe-instances --instance-ids "$INSTANCE_ID" \
  --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)
seen=$(openssl s_client -connect "$ip:8792" </dev/null 2>/dev/null |
  openssl x509 -pubkey -noout | openssl pkey -pubin -outform der | sha256sum | cut -d' ' -f1)
[ -n "$expected" ] && [ "$seen" = "$expected" ] ||
  { echo "https://$ip:8792 answers with pin ${seen:-none}, not ${expected:-none}" >&2; exit 1; }
echo "live at https://$ip:8792, pin $seen"
