# C2PA capture, sign and verify pipeline

Local end-to-end C2PA pipeline for the TD8 / P27 thesis, with a placeholder
custom assertion (`edu.utdt.td8.zkloc`) that carries the shape the real
ZK location + transformation assertion will eventually have.

Closes [C2P-19](https://linear.app/c2pa-tesis/issue/C2P-19/run-a-c2pa-capture-sign-and-verify-pipeline-locally).

## Requirements

- macOS or Linux
- `gh` (GitHub CLI), authenticated, to fetch the pinned `c2patool` release and the sample certs
- `python3`, used only to assert the round trip in `run.sh`

## Setup

```bash
./scripts/setup.sh
export PATH="$HOME/.local/bin:$PATH"
```

Installs `c2patool` at a pinned version and downloads the SDK sample
certificates and test image. Idempotent.

## Run

```bash
./scripts/run.sh
```

Optionally with your own input and manifest:

```bash
./scripts/run.sh path/to/image.jpg manifests/zkloc-placeholder.json
```

Outputs land in `out/`:

| File | What it is |
| --- | --- |
| `out/signed.jpg` | the input image with an embedded C2PA manifest |
| `out/read-back.json` | the manifest read back out of that image |
| `out/sign.json` | c2patool's report from the signing step |

## What this demonstrates

| C2P-19 criterion | Status |
| --- | --- |
| A documented command runs sign and verify end to end | `./scripts/run.sh` |
| A custom assertion with placeholder content survives the round trip and is readable | `edu.utdt.td8.zkloc` recovered with all nested fields intact |
| An off-the-shelf C2PA validator is run against the output, result recorded | see [Validation results](#validation-results) |
| Commands and tool versions in the README | below |

## Versions

Recorded on the machine where this was last run.

| Component | Version |
| --- | --- |
| c2patool | 0.27.16 |
| Python | 3.14.3 |
| gh | 2.63.2 |
| OS | macOS 26.5.2, arm64 |
| Certificates | `cli/sample` from `contentauth/c2pa-rs`, ES256 |

## Validation results

| Validator | Result | Notes |
| --- | --- | --- |
| `c2patool` (local) | `validation_state: Valid` | one informational issue: `signingCredential.untrusted` |
| verify.contentauthenticity.org | `Unrecognized` | manifest parsed and displayed; the status refers to the signing certificate, not to our assertion |

Adobe's public verifier read the manifest and surfaced our claim generator
(`td8-zkloc-pipeline 0.1.0`), our `c2pa.actions` entry (`Created`), the
`digitalCapture` source type, and the signer identity (`C2PA Test Signing
Cert`). It marked the asset `Unrecognized`, which refers to the signing
certificate not being on its trust list.

**It did not display our custom assertion.** The file validates and the
assertion is not rejected, but a mainstream verifier renders only the
assertions it knows and ignores the rest. See
[Custom assertions are invisible to third-party verifiers](#custom-assertions-are-invisible-to-third-party-verifiers).

`signingCredential.untrusted` is expected and not a defect. We sign with the
SDK's public sample certificates, which are deliberately not on any C2PA trust
list. Hardware-backed capture attestation is declared out of scope for this
thesis, so a real trust chain is not something we pursue.

## Gotchas

**`C2PA_PRIVATE_KEY` and `C2PA_SIGN_CERT` take PEM contents, not file paths.**
Passing a path fails with `Invalid certification data ... Input type: PEM`,
which reads like a malformed certificate rather than a wrong argument. `run.sh`
uses `$(cat ...)` for this reason.

**Certificates are not committed.** `certs/` is gitignored. The sample private
key is public and worthless, but committing a file named `*_private.key` to a
public repository trips secret scanners and sets a bad habit. `setup.sh`
fetches them instead.

## Early finding: manifest size

For the placeholder assertion, whose proof fields are `null`:

```
Manifest store size = 63730 (50.80% of file size 125462)
```

Most of that is the certificate chain and the generated thumbnail, not our
assertion. It is still worth watching, because real proofs are far from free:
a Groth16 proof is a couple of hundred bytes, but a Brakedown proof is orders
of magnitude larger. If the composition path chosen at the
[C2P-24 gate](https://linear.app/c2pa-tesis/issue/C2P-24) produces large proofs,
whether they fit in a C2PA manifest at all becomes a real question, and one
worth answering before M3 rather than during it.

## Custom assertions are invisible to third-party verifiers

Adobe's verifier accepted the signed file and showed the standard parts of the
manifest, but showed nothing at all for `edu.utdt.td8.zkloc`. It does not
reject the assertion; it simply does not know how to render it, so it is
dropped from the summary view.

This has a consequence for the newsroom proof of concept. The story ends with
a reader verifying the image, and that reader cannot use
contentcredentials.org to see the location claim, because it will not be
shown. Proving regional provenance to a reader therefore requires a verifier we
write ourselves.

Worth scoping into [C2P-32](https://linear.app/c2pa-tesis/issue/C2P-32) and
[C2P-33](https://linear.app/c2pa-tesis/issue/C2P-33) now rather than
discovering it during M3.

## The placeholder assertion

`manifests/zkloc-placeholder.json` carries `edu.utdt.td8.zkloc` with the shape
of the eventual real assertion and dummy values throughout:

- `commitment`: algorithm, what it covers, and the value
- `location_proof`: H3 scheme, resolution, cell, proof system and field
- `transformation_proof`: the operations proven, proof system and field
- `binding`: the claim that both proofs reference the same commitment

Everything is marked `TBD` or `null`. Nothing here is cryptographically
meaningful yet; the point is that the extension point works and that the schema
has a shape to argue about now rather than in November.

## Limitations

- The signature uses public sample certificates. This is not a trust chain.
- No zero-knowledge proof is generated or verified. The proof fields are `null`.
- Only JPEG has been exercised.
- `setup.sh` supports macOS and Linux x86_64. Other targets need the matching
  release asset name.
