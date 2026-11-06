# deploy

`provenance serve` on EC2, in two environments:

| | prod | dev |
| --- | --- | --- |
| Runs | the last commit on `main` | the last commit on `dev` |
| Address | an Elastic IP, so a paired app keeps working | a new public IP at every start |
| Hours | `office`: 9:00 to 21:00 on weekdays | `stop-nightly`: off at 21:00, started by each deploy |

Each environment has its own TLS key, device key, trusted phones and
captures, in `/var/lib/provenance`. A phone paired with dev cannot publish to
prod. Both servers see the original photo, like the laptop, so they are for
test photos.

## Branches

1. Work branches start from `dev` and come back to it in a PR. The merge deploys to dev.
2. A PR from `dev` to `main` deploys to prod. Merge it with a merge commit, not a squash,
   or `main` gets commits `dev` lacks.
3. A fix prod needs right away goes in a PR to `main`, then `main` is merged into `dev`.

## What runs

- `.github/workflows/build-release.yml` builds a release in an `amazonlinux:2023` container
  with `build.sh`: `provenance`, `location-proof`, `web/dist` and the scripts below.
  Both binaries link against glibc, so they are built on the instance's OS.
- `.github/workflows/release.yml`, on a push to `main` or `dev`, runs `ship.sh` in the
  GitHub environment `prod` or `dev`. It takes an AWS role through OIDC, uploads the
  release, starts the instance when it is stopped, and runs `release.sh` on it through
  Session Manager. Then it checks from outside that port 8792 answers with the TLS key
  the instance reports.
- `release.sh` runs `provenance setup` with the release's `location-proof`, points
  `/opt/provenance/current` at the release and restarts `provenance.service`. Then it runs
  `provenance demo`, which proves, publishes and verifies a whole capture. When that fails,
  it puts the previous release back and fails the deploy.
- `.github/workflows/deploy.yml` checks this folder: cfn-lint, shellcheck and actionlint.
  It also runs `release.sh` on the runner, once with a good release and once with a release
  whose server cannot start, which must be refused while the good one keeps serving.

## Setting it up once

In AWS, as an administrator of the account (us-east-2):

```bash
aws cloudformation deploy --stack-name provenance-account --template-file deploy/account.yml
aws cloudformation deploy --stack-name provenance-prod --template-file deploy/env.yml \
  --capabilities CAPABILITY_IAM \
  --parameter-overrides Env=prod Hours=office VpcId=<vpc> SubnetId=<public subnet>
aws cloudformation deploy --stack-name provenance-dev --template-file deploy/env.yml \
  --capabilities CAPABILITY_IAM \
  --parameter-overrides Env=dev Hours=stop-nightly VpcId=<vpc> SubnetId=<public subnet>
```

`Hours=always` keeps an environment on around a demo.

In GitHub, as an administrator of the repository:

- Create the `dev` branch from `main`. Protect it like `main`.
- Create the environments `prod` and `dev`. Under "Deployment branches", allow only `main`
  in `prod` and only `dev` in `dev`. The deploy roles only trust their own environment.
- In each environment, set these variables:
  - `AWS_DEPLOY_ROLE`: the stack output `DeployRoleArn`.
  - `AWS_INSTANCE_ID`: the stack output `InstanceId`.
  - `AWS_RELEASES_BUCKET`: the output `ReleasesBucket` of `provenance-account`.

## Pairing

`/screen` and the pairing token only answer on the instance itself. To open them from a
laptop with access to the account:

```bash
aws ssm start-session --target <instance id> --document-name AWS-StartPortForwardingSession \
  --parameters portNumber=8791,localPortNumber=8791
```

Then open `http://localhost:8791/screen` and scan the QR code with the app.
