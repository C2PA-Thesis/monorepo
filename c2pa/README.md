# C2PA custom assertion round trip

This folder preserves the C2P-19 sign and read-back check and now uses the real
version 1 shape of `edu.utdt.td8.zkloc`. The tracked example has schema-valid
fields and one-byte proof placeholders. It tests C2PA extension handling only.
Its device signature and proof bytes are deliberately not valid cryptographic
artifacts. The full pipeline creates real artifacts through `pipeline/run.sh`.

## Setup and run

Requirements are macOS or Linux, Python 3, and an authenticated GitHub CLI.

```bash
./scripts/setup.sh
export PATH="$HOME/.local/bin:$PATH"
./scripts/run.sh
```

The setup script installs c2patool 0.27.16 and downloads the c2pa-rs SDK sample
certificate, signing key, trust anchors, and image. The sample C2PA key signs
the editor manifest. It is not the device key used by the binding pipeline.

`run.sh` signs the fixture, reads the manifest back, requires
`validation_state: Valid`, and checks that the version 1 custom assertion is
present. Outputs are written to `out/`.

| File | Contents |
| --- | --- |
| `out/signed.jpg` | Image with an embedded C2PA manifest |
| `out/read-back.json` | Manifest read from the signed image |
| `out/sign.json` | c2patool signing report |

## C2P-19 baseline

The original placeholder round trip established these results with c2patool
0.27.16 on macOS arm64:

| Validator | Result | Meaning |
| --- | --- | --- |
| c2patool | `validation_state: Valid` | The signed asset and custom assertion parsed locally |
| verify.contentauthenticity.org | `Unrecognized` | The sample signing certificate was not trusted, but the manifest parsed |

The public interface displayed the standard claim generator, action, digital
source type, and sample signer, but did not render the unknown custom assertion.
That remains the reason this repository supplies its own reader-side verifier.

For the old placeholder assertion, c2patool reported a 63,730-byte manifest
store in a 125,462-byte JPEG. That result is historical baseline data, not a
measurement of the version 1 proof bundle.

## Version 1 assertion example

`manifests/zkloc-example.json` contains the same shape enforced by
`pipeline/schemas/assertion.schema.json`:

- the device-signed receipt, including `captured_at`
- the public H3 cell and canonical face/IJK tuple
- the Groth16 location proof and its public inputs
- the HyperVerITAS PST proof metadata and checksum-bound inline member
- an explicit `device-signed-public-input-pair` binding description

Run the schema and fixture tests from the repository root:

```bash
PYTHONPATH=pipeline pipeline/.venv/bin/python -m unittest \
  discover -s pipeline/tests -t pipeline -v
```

## Signing material

`C2PA_PRIVATE_KEY` and `C2PA_SIGN_CERT` contain PEM data, not paths. The scripts
load the files into those environment variables. The downloaded SDK
certificates live under ignored `certs/`.

The local validator can report `signingCredential.untrusted` because the SDK
sample certificate is not a production trust anchor. The pipeline still
requires `validation_state` to be `Valid` before it evaluates the custom
assertion.

## Scope

This folder demonstrates that c2patool preserves and reads back an unknown
custom assertion. It does not verify the example's dummy receipt or proof
bytes. `pipeline/verify.py` verifies artifacts created by the real pipeline.
