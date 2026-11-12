# deploy

`provenance serve` on EC2, in two environments:

| | prod | dev |
| --- | --- | --- |
| Runs | the last commit on `main` | the last commit on `dev` |
| Address | an Elastic IP | an Elastic IP |
| Hours | `office`: 9:00 to 21:00 on weekdays | `stop-nightly`: off at 21:00, started by each deploy |

Each environment has its own TLS key, device key, trusted phones and
captures, in `/var/lib/provenance`. That directory is a volume of its own
(`Data` in `env.yml`), kept when the stack or the instance is deleted, so the
TLS key the app has built in survives a replaced instance. A phone registered
with dev cannot publish to prod. Both servers see the original photo, like the
laptop, so they are for test photos.

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
- `release.sh` mounts the data volume at `/var/lib/provenance`, formatting it the first
  time. When an older release left data on the root volume, it is copied over and the
  old directory is kept as `/var/lib/provenance.root-volume`. Then it runs
  `provenance setup` with the release's `location-proof`, points
  `/opt/provenance/current` at the release and restarts `provenance.service`. Then it runs
  `POST /api/v2/challenge`, which must answer, and `provenance demo`, which proves,
  publishes and verifies a whole capture. When either fails,
  it puts the previous release back and fails the deploy.
- `.github/workflows/deploy.yml` checks this folder: cfn-lint, shellcheck and actionlint.
  It also runs `release.sh` on the runner, once with a good release and once with a release
  whose server cannot start, which must be refused while the good one keeps serving.

## Setting it up once

In AWS, as an administrator of the account (us-east-2). GitHub's OIDC provider
comes first, unless the account already has one for
`token.actions.githubusercontent.com`:

```bash
aws iam create-open-id-connect-provider \
  --url https://token.actions.githubusercontent.com --client-id-list sts.amazonaws.com
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

Once per account, for Apple's risk metric: create a key with DeviceCheck enabled in the
Apple Developer account (Certificates, Identifiers & Profiles, Keys), then store it and its
Key ID in SSM Parameter Store, with the Team ID. Both environments read the same three
parameters:

```bash
aws ssm put-parameter --region us-east-2 --name /provenance/apple/app-attest-key \
  --type SecureString --value file://AuthKey_<KEYID>.p8
aws ssm put-parameter --region us-east-2 --name /provenance/apple/app-attest-key-id \
  --type String --value <KEYID>
aws ssm put-parameter --region us-east-2 --name /provenance/apple/team-id \
  --type String --value <TEAMID>
```

Without the key the servers still register phones, without checking the metric, and the
journal says so (`journalctl -u provenance`). Without `team-id`, `serve.sh` uses
`678K8MRCKG`.

## Registration

Both servers run `provenance serve --open-registration` with the App ID
`<team-id>.io.github.c2pa-thesis.capture` (`serve.sh`). A phone registers without a QR
code:

1. The app connects to the address and TLS pin built into it (`ios/ProvenanceCapture/Resources/servers.json`)
   and reads `GET /api/v2/server` for the parameter digests.
2. It asks for a single-use challenge (`POST /api/v2/challenge`), attests a new App Attest
   key over the challenge and its receipt key, and sends both to `POST /api/v2/register`.
3. The server verifies the attestation, accepts validation categories 2 (TestFlight),
   3 (development) and 4 (App Store) in both App Attest environments, refuses an App Attest
   key that registered another receipt key, and with the DeviceCheck key asks Apple for the
   risk metric and refuses more than 10 attested keys on the device in 30 days.

A trusted key then means that an instance of the team's app on Apple hardware registered
it, by Apple's account, not that someone at the server paired it. Any build signed by the
team registers, modified or not. While registration is open each device may send 10
captures an hour, and uploads are refused while 4 captures wait for a proof.

To build the app for these servers, fill `servers.json` with each environment's address
(the stack output `Address`, as `https://<ip>:8792`) and the pin the deploy prints
(`live at https://<ip>:8792, pin <pin>`).

## Pairing with the QR code

`/screen` and the pairing token only answer on the instance itself. To open them from a
laptop with access to the account:

```bash
aws ssm start-session --target <instance id> --document-name AWS-StartPortForwardingSession \
  --parameters portNumber=8791,localPortNumber=8791
```

Then open `http://localhost:8791/screen` and scan the QR code with the app.

## Not verified

- The data volume on a real instance: the mount, the copy from the root volume, and that a
  replaced instance gets the same volume back. The runner test has no such device and skips
  it.
- That the instance role reads the SecureString with `ssm:GetParameter` alone, through the
  default `aws/ssm` key.
- That `678K8MRCKG`, the Team ID in SSM, is the paid Developer Program team's. Xcode shows
  the same ID for the Personal Team. The App ID, and so every attestation's RP ID, depends
  on it.
- Registration from an iPhone against either server: no App Attest attestation and no Apple
  receipt with a risk metric has reached one yet.
