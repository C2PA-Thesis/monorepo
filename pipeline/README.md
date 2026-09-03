# Private location and edit-proof pipeline

This command line proof of concept publishes a 512 by 512 PNG with one C2PA
custom assertion, `edu.utdt.td8.zkloc`. The assertion binds two independent
proofs to one capture through a device signature over their public values.

This is a device-signed pair of public inputs. It is not a single field
commitment shared by both proof systems.

## Critical ZKLP soundness limitation

The current upstream FP32 circuit accepts trigonometric precomputation through
a gnark hint. Its constraints check identities among the hint outputs, but do
not bind those outputs back to the secret latitude and longitude. An honest
prover follows the intended computation. A malicious prover is not forced to.

The current PoC therefore verifies the C2PA wiring, device-signed binding,
published-image proof, and honest-prover location flow. It is not yet a
malicious-prover-sound private location attestation. The location claim must
not be treated as production proof until the hint outputs are constrained to
the coordinates or replaced by fully constrained computation.

## One-command run

The repository needs the `editproof/` and `locproof/` submodules. Setup also
requires Git, Go, a Rust nightly toolchain, Python 3, and an authenticated
GitHub CLI for the pinned c2patool download.

```bash
./pipeline/run.sh
```

`pipeline/setup.sh` is idempotent. It creates `pipeline/.venv`, installs the
pinned Python requirements, builds both proof CLIs, caches PST and Groth16
parameters under `pipeline/generated/`, creates a separate P-256 demo device
key pair, and records exact versions in
`pipeline/generated/tool-versions.json`. After setup, the pipeline does not
need network access.

To reuse the cached setup:

```bash
PIPELINE_SKIP_SETUP=1 ./pipeline/run.sh
```

The public demo point is latitude `-34.5478`, longitude `-58.4462`, near
Universidad Torcuato Di Tella in Buenos Aires. Its resolution-7 H3 cell is
`87c2e3020ffffff`. These coordinates are committed test data. The demo tests
non-disclosure by the published artifact, not secrecy of the source tree.

## Commands

The run script executes these interfaces:

```bash
pipeline/.venv/bin/python pipeline/capture.py fixtures/generated/demo-input.jpg \
  --lat -34.5478 --lon -58.4462 \
  --device-key fixtures/generated/device-private.pem \
  --out pipeline/out/capture

pipeline/.venv/bin/python pipeline/crop.py \
  --capture pipeline/out/capture --out pipeline/out/edit

pipeline/generated/bin/zkloc prove --params pipeline/generated/zkloc-groth16 \
  --secrets pipeline/out/capture/secrets.json \
  --region fixtures/region-utdt-res7.json --out pipeline/out/pi-loc.json

editproof/hyperveritas_impl/target/release/hv prove-crop \
  --params pipeline/generated/hv-pst \
  --original pipeline/out/capture/original.json \
  --edited pipeline/out/edit/edited.json \
  --fingerprint pipeline/out/capture/fingerprint.json \
  --out pipeline/out/pi-img

pipeline/.venv/bin/python pipeline/package.py \
  --receipt pipeline/out/capture/receipt.json \
  --region fixtures/region-utdt-res7.json \
  --pi-loc pipeline/out/pi-loc.json --pi-img pipeline/out/pi-img \
  --edited pipeline/out/edit/edited.png --out pipeline/out/signed.png

pipeline/.venv/bin/python pipeline/verify.py pipeline/out/signed.png \
  --region fixtures/region-utdt-res7.json \
  --device-public-key fixtures/generated/device-public.pem \
  --hv-params pipeline/generated/hv-pst \
  --zkloc-vk pipeline/generated/zkloc-groth16
```

`capture.py` accepts an optional RFC3339 `--captured-at`. The default is the
current UTC time. This value is signed by the device, but it is device-asserted
time, not a trusted time source.

## Data contracts

All contracts use JSON Schema draft 2020-12 and `schema_version: 1`.

| Contract | Important fields |
| --- | --- |
| Fingerprint | Ajtai/ChaCha8 algorithm, BLS12-381 scalar field, 128 canonical hex elements per R/G/B channel |
| Envelope | MiMC on BN254, explicit coordinate encoding, canonical field element |
| Receipt | Fingerprint, envelope, dimensions, `captured_at`, device key ID, P-256 signature |
| Secrets | Relative original image path, coordinates, fresh salt, capture time |
| Region | H3 cell plus resolution, face, i, j, k |
| Location proof | Groth16 proof, exact public envelope and region, parameter digests |
| Image proof | PST metadata plus base64 proof members with size and SHA-256 checksums |
| Assertion | Receipt, both proofs, region, and binding description |

Field elements are lowercase `0x` strings with 64 hexadecimal digits. The
location salt is 32 bytes with its high byte set to zero, so it is at most 248
bits and is a canonical BN254 scalar input. Private secrets, device keys,
parameters, proofs, and run outputs are ignored by Git.

HyperVerITAS image JSON uses `rows` for width and `cols` for height, matching
the upstream prototype. Channels are flattened in row-major pixel order. At
capture, Pillow converts the source to RGB, resizes it to 1024 by 512, and
writes a lossless PNG and matching JSON. The only edit is the left half,
coordinates `(0, 0)` through `(512, 512)`.

## Receipt canonicalization

The signed payload contains exactly these keys:

```text
schema_version, fingerprint, envelope, rows, cols, captured_at, device_key_id
```

It is encoded as UTF-8 JSON with keys sorted, no insignificant whitespace, and
non-ASCII characters preserved. SHA-256 hashes those bytes. ECDSA P-256 signs
that digest with SHA-256 prehash semantics. `device_key_id` is the lowercase
SHA-256 digest of the public SubjectPublicKeyInfo DER bytes. The receipt does
not carry a public key. The verifier requires the trusted public key as an
explicit argument.

## Verification

The verifier first requires c2patool to report `validation_state: Valid` and
requires exactly one custom assertion. It then runs three checks:

1. Validate the receipt schema, trusted key ID, and device signature. This
   includes the signed `captured_at` value.
2. Decode pixel data from the actual C2PA-signed PNG, rebuild HyperVerITAS JSON,
   checksum the inline proof members, and verify the crop proof against the
   receipt fingerprint.
3. Require the embedded region to equal the caller's requested region and
   verify the Groth16 proof against the receipt envelope and that region.

An expected staged failure exits nonzero and prints exactly `check 1`,
`check 2`, or `check 3`. A C2PA or assertion-schema failure is reported before
the staged proof checks.

## Tests

Fast tests run with:

```bash
PYTHONPATH=pipeline pipeline/.venv/bin/python -m unittest \
  discover -s pipeline/tests -t pipeline -v
```

They validate every schema, validate the tracked C2PA example, and exercise the
packaging and verification logic for the named negative cases. Those tests use
stubbed c2patool, HyperVerITAS, and ZKLP process boundaries. They test binding
and failure routing, but they do not establish proof-system or C2PA integration.

After the valid proof run, `pipeline/run.sh` invokes
`pipeline/integration_negatives.py`. This real integration runner reuses the
valid proof artifacts, creates a fresh second capture without generating
another expensive image proof, and produces these results:

| Mutation | Expected | Real observed result |
| --- | --- | --- |
| Change one signature byte | `check 1` | `check 1` |
| Replace the fingerprint without signing | `check 1` | `check 1` |
| Replace the fingerprint and sign with the trusted demo key | `check 2` | `check 2` |
| Prove against canonical neighbor `87c2e3021ffffff` | Prover refusal or `check 3` | Prover refused because the coordinate does not map to the region |
| Swap in a second real capture receipt | `check 2` or `check 3` | `check 3` |
| Change a published pixel without a proof | `check 2` | `check 2` |

Every mutated asset is signed into a new C2PA PNG. The runner requires
`validation_state: Valid` before accepting the expected custom-verifier
failure. A wrong-region proving refusal creates no mutated asset. Exact results
and relative artifact paths are written to
`pipeline/out/negative/results.json`. The full GitHub Actions integration job
runs the same positive and negative entrypoint.

## Measurements

The run writes wall timings to `pipeline/out/timings.tsv` and prints sizes for
the PST proof, Groth16 proof, C2PA manifest store, and signed PNG.

| Measurement | Result |
| --- | --- |
| Capture and both commitments | 9.742076 s wall |
| Fixed crop and JSON conversion | 0.391120 s wall |
| HyperVerITAS PST prover command | 153.009927 s wall |
| HyperVerITAS PST verifier command | 3.807 s core, 3.82 s wall |
| ZKLP Groth16 prover command | 0.539033 s wall |
| ZKLP Groth16 verifier subprocess | 0.011481 s median, 30 runs |
| P-256 signature verification only | 0.000054 s median, 100 runs |
| Complete receipt check | 0.005427 s median, 100 runs |
| C2PA package command | 0.199918 s wall |
| Complete reader-side verifier | 4.232876 s wall |
| Six-case binding-negative integration | 26.159726 s wall |
| PST proof size | 36,692 bytes |
| Groth16 compressed proof size | 196 bytes, 857-byte proof JSON |
| C2PA manifest store | 205,776 bytes |
| Final signed PNG | 317,491 bytes |

These results came from a real `pipeline/run.sh` on macOS 26.6.2 arm64. The
run used Python 3.9.6, Pillow 11.3.0, cryptography 45.0.6, jsonschema 4.25.1,
h3 4.3.1, rustc 1.100.0-nightly (2026-09-01), Go 1.26.5, c2patool 0.27.16,
hv 0.1.0, and zkloc-poc 0.1.0. It built directly from pinned submodule commits
`798554ff32f4341a9458d07c0160981c35310eaf` (HyperVerITAS) and
`cc8f1c7451561d05391dc604ac36e3d0dde1bae7` (ZKLP).

The PST CLI separately printed `prover time: 15.195 seconds`. The table uses
the full command wall time, which includes every phase required to produce the
serialized proof. The signature microbenchmark uses
`pipeline/benchmark_receipt.py`; the complete check includes schema validation,
key ID matching, canonicalization, hashing, and ECDSA verification.

The Groth16 setup produced 21,161 constraints, a 3,141,321-byte R1CS, a
4,847,803-byte proving key, and a 684-byte verifying key. Setup files are
cached and are not included in per-capture proof sizes.

## Threats and limitations

- The capture device is trusted to report pixels, coordinates, and time
  honestly. This PoC does not provide hardware attestation or trusted time.
- The current ZKLP FP32 hint outputs are not constrained back to the secret
  coordinates. Location verification demonstrates honest-prover behavior, not
  malicious-prover soundness.
- The generated P-256 identity is a local trust-store stand-in, not a
  certificate chain. The C2PA editor key is separate.
- PST setup is a PoC trusted setup inherited from the prototype.
- Only a 1024 by 512 input and fixed 50 percent left crop are supported.
- Only the PST HyperVerITAS variant and Groth16 ZKLP variant are packaged.
- The exact coordinates remain in the ignored prover secrets. The H3 cell,
  fingerprint, envelope, dimensions, device key ID, and capture time are public.
- A valid proof says that the witness satisfies the implemented relations. It
  does not establish that a physical GPS sensor was honest.

## Fact-checker questions

The artifact answers these questions independently:

1. Does the trusted device key validate the exact fingerprint, envelope,
   dimensions, and asserted capture time?
2. Do the pixels in the signed PNG verify as the fixed crop of the fingerprinted
   original?
3. Does the honest-prover ZKLP path accept the committed coordinate for the
   requested global H3 cell? (The hint limitation above prevents a stronger
   malicious-prover claim.)
4. Is the requested region exactly the region embedded in the C2PA assertion?
5. Did c2patool validate the signed asset before custom verification began?

It does not answer who controlled the device, whether its sensors were honest,
or whether `captured_at` came from trusted hardware.
