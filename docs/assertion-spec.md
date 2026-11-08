# `io.github.c2pa-thesis.zkloc` assertion specification

Version 6. Label: `io.github.c2pa-thesis.zkloc`.

Legacy label: `edu.utdt.td8.zkloc` (read but no longer written; files
published under it keep verifying).

A custom C2PA assertion carrying two zero-knowledge proofs and the
device receipt that binds them.  The assertion sits inside a standard
C2PA manifest alongside `c2pa.actions` (with `c2pa.created`,
`c2pa.cropped`, and optionally `c2pa.orientation`).  A reader that
knows this specification can verify the complete claim; a reader that
does not simply ignores the assertion and sees only the standard C2PA
metadata.

## Namespace

C2PA labels start with the domain of whoever defines them: "Entity-
specific namespaces shall begin with the Internet domain name for the
entity" (C2PA 2.2, §6.2.1).  The namespace is `io.github.c2pa-thesis`,
from `c2pa-thesis.github.io`, the domain the project's GitHub
organization controls.  The previous label `edu.utdt.td8.zkloc` used
the university's domain, which is not this project's to define.

## Claim

A published PNG is a stated rectangle of an original (optionally
turned by a multiple of 90 degrees) captured by a trusted device
whose sealed hints map to a stated H3 cell.  The original pixels and
the exact coordinate are not published.

## Encoding

The assertion is serialized to CBOR by `c2pa-rs` when the manifest is
signed and deserialized back when it is read.  Its JSON representation
(as returned by `provenance inspect`) is the canonical reference for
field names, types and encodings.  All string-encoded binary values
use the encoding stated below; no implicit conversions apply.

## Schema

```jsonc
{
  "version": 6,

  "receipt": {
    "fingerprint": {
      "algorithm": "hyperveritas-ajtai-chacha8-v1",
      "size": { "width": <uint>, "height": <uint> },
      "R": [ <hex-scalar>, ... ],   // 128 elements
      "G": [ <hex-scalar>, ... ],   // 128 elements
      "B": [ <hex-scalar>, ... ]    // 128 elements
    },
    "envelope":    <hex-scalar>,
    "captured_at": <rfc3339>,
    "device":      <hex-string>,
    "signature":   <base64-string>
  },

  "cell":           <h3-cell>,
  "crop":           { "x": <uint>, "y": <uint>, "width": <uint>, "height": <uint> },
  "turn":           <uint>,
  "location_proof": <base64-string>,
  "image_proof":    <base64-string>
}
```

No additional fields are accepted (`deny_unknown_fields` in the Rust
deserializer).

### Top-level fields

| Field            | Type   | Description |
|------------------|--------|-------------|
| `version`        | uint   | Schema version.  Must be `6` (or `5`, which is accepted when `turn` is 0). |
| `receipt`        | object | Device-signed receipt binding the fingerprint and the envelope. |
| `cell`           | string | H3 cell index the location proof claims, e.g. `"87c2e3020ffffff"`. |
| `crop`           | object | Rectangle of the original these published pixels are claimed to be. |
| `turn`           | uint   | Quarter turns clockwise applied after the crop: 0 (none), 1 (90°), 2 (180°), 3 (270°).  Absent in version 5; defaults to 0. |
| `location_proof` | string | Base64 (RFC 4648 §4, standard alphabet, with padding) encoding of the 196-byte BN254 Groth16 proof. |
| `image_proof`    | string | Base64 encoding of the HyperVerITAS PST crop proof (approximately 36 KB). |

### `receipt`

The receipt is the device's signed attestation that links both proofs
to a single capture event.  It is the **only** element binding the crop
proof and the location proof together.

| Field          | Type   | Description |
|----------------|--------|-------------|
| `fingerprint`  | object | Lattice (Ajtai) hash of the original pixels. |
| `envelope`     | string | MiMC commitment to the coordinate, trigonometric hints and a fresh salt, as a BN254 scalar.  Hex-encoded with `0x` prefix. |
| `captured_at`  | string | RFC 3339 timestamp asserted by the device (not by a trusted clock). |
| `device`       | string | SHA-256 of the device's P-256 public key in SubjectPublicKeyInfo DER, hex-encoded (64 lowercase hex chars, no prefix). |
| `signature`    | string | Base64 DER-encoded ECDSA P-256 signature over the signed bytes (see below). |

#### `receipt.fingerprint`

| Field       | Type     | Description |
|-------------|----------|-------------|
| `algorithm` | string   | Must be `"hyperveritas-ajtai-chacha8-v1"`. |
| `size`      | object   | `{"width": <W>, "height": <H>}` — the original's dimensions.  The product W × H must not exceed 2^19 (524 288). |
| `R`         | string[] | 128 BLS12-381 scalar field elements, hex-encoded with `0x` prefix (64 hex digits each). Red channel. |
| `G`         | string[] | Same, green channel. |
| `B`         | string[] | Same, blue channel. |

The fingerprint is the product `A · pixels` where `A` is a
128 × (2^19) matrix over BLS12-381 derived deterministically from
ChaCha8 with a zero seed, and `pixels` is the column vector of pixel
values cast to the scalar field, zero-padded to 2^19 elements.  One
field element per row per channel, 384 elements total.

Originals smaller than 2^19 pixels share the same parameters (the
matrix and the KZG structured reference string): smaller images are
padded with zeros to fill the full polynomial domain.

The fingerprint is **deterministic**: anyone holding the original image
can recompute it and check whether it matches.

#### Signed bytes

The device signs the compact (no whitespace) JSON serialization of
this object, in this field order:

```json
{"fingerprint":<fingerprint>,"envelope":"<envelope>","captured_at":"<captured_at>","device":"<device>"}
```

where `<fingerprint>` is the fingerprint object with its own canonical
field order: `algorithm`, `size` (with `width` before `height`), `R`,
`G`, `B`.  The signature covers all four fields; `signature` itself is
not included.

A verifier recomputes this JSON from the four fields and checks the
ECDSA P-256 signature against the device's public key, identified by
the `device` field (the key id).

### `crop`

| Field    | Type | Description |
|----------|------|-------------|
| `x`      | uint | Horizontal offset from the top-left corner of the original, in pixels. |
| `y`      | uint | Vertical offset from the top-left corner. |
| `width`  | uint | Width of the rectangle.  Must be > 0 and `x + width <= original width`. |
| `height` | uint | Height of the rectangle.  Must be > 0 and `y + height <= original height`. |

The crop rectangle is a public input of the image proof.

### `turn`

Quarter turns clockwise applied to the crop before publication:

| Value | Effect |
|-------|--------|
| 0     | No rotation (default). |
| 1     | 90° clockwise. |
| 2     | 180°. |
| 3     | 270° clockwise (= 90° counter-clockwise). |

When `turn` is non-zero, the C2PA actions include `c2pa.orientation`
alongside `c2pa.created` and `c2pa.cropped`.

The published image dimensions are: if `turn` is 0 or 2, the crop's
`width × height`; if 1 or 3, `height × width`.

The turn is hashed into the crop proof's Fiat-Shamir transcript.

### `location_proof`

Base64 encoding of a BN254 Groth16 proof (196 bytes decoded).

The proof establishes that the hints sealed in `receipt.envelope` map
to the H3 cell stated in `cell`.  The circuit maps the hints to an H3
cell index and checks that the result matches.

**Soundness caveat:** the circuit does not constrain the trigonometric
hints back to the sealed coordinate.  The proof that the hints map to
the cell holds unconditionally; the claim that the *coordinate* lies
in the cell holds only for an honest prover that computed the hints
from its coordinate (see Known limits).

The verifier needs the Groth16 verifying key (generated during setup)
and two public inputs: the `envelope` scalar and the cell's
(face, i, j, k, resolution) tuple, which are derived from the `cell`
string via the H3 library.

### `image_proof`

Base64 encoding of a HyperVerITAS PST proof.

The proof establishes that the published pixels are the rectangle
`crop` of the original image (turned by `turn` quarter turns) whose
channel polynomials commit under the lattice hash to
`receipt.fingerprint`, and that every pixel value is in [0, 255].

The crop proof's Fiat-Shamir transcript binds the proof to its full
statement: the crop rectangle, the turn, the original's size, and the
fingerprint.

#### Wire format (`HVPST002`)

The decoded bytes follow this layout:

| Offset | Content |
|--------|---------|
| 0      | Magic: `HVPST002` (8 bytes, ASCII). |
| 8      | Length-prefixed frames of arkworks BLS12-381 compressed canonical serializations. |

Each frame is a little-endian `u64` byte count followed by that many
bytes.  The frames, in order:

1. **Commitments** — 12 KZG commitments (3 image channels + 9 auxiliary polynomials for range and crop checks).
2. **Hash sumchecks** — 3 sumchecks (one per channel), each: one frame of challenge-point field elements, then a `u32` round count, then one frame per round.
3. **Range sumchecks** — 3 sumchecks, same structure, 20 variables (NUM_VARS + 1).
4. **Crop sumchecks** — 3 sumchecks, same structure, 19 variables.
5. **Image batch opening** — one sumcheck (19 vars) + one frame of 9 evaluations + one KZG proof frame.
6. **Range batch opening** — one sumcheck (20 vars) + one frame of 33 evaluations + one KZG proof frame.

Each sumcheck's challenge point has as many elements as its variable
count (19 or 20).  Round polynomials have degree at most 3, so each
round frame holds 1–8 field elements.

The verifier needs the PST verifying parameters (generated during
setup).

## Verification procedure

A reader verifies a signed PNG in four sequential checks, stopping at
the first rejection.

### Check 1: C2PA manifest

Read the C2PA manifest with `c2pa-rs` (or any conforming reader).
Accept when the validation state is `Valid` or `Trusted`.  Extract the
single assertion labeled `io.github.c2pa-thesis.zkloc` (or the legacy
`edu.utdt.td8.zkloc`).  Reject if none or more than one is present.
Accept version 6 or version 5 (when `turn` is 0).

### Check 2: Device receipt

Recompute the signed bytes from the receipt's `fingerprint`, `envelope`,
`captured_at` and `device` fields.  Look up the device's P-256 public
key by `device` (the SHA-256 of its SubjectPublicKeyInfo DER) in the
set of trusted keys.  Verify the ECDSA signature.

### Check 3: Crop proof

Decode `image_proof` from Base64.  Read the published pixels from the
signed PNG.  Verify the HyperVerITAS PST proof against the verifier
parameters, the published image, the crop rectangle, the turn and the
fingerprint from the receipt.

This check establishes: the published pixels are the stated rectangle
(turned by `turn`) of the image whose channel polynomials the receipt
fingerprints, and every pixel is in [0, 255].

### Check 4: Location proof

Derive the (face, i, j, k, resolution) tuple from the `cell` string
using the H3 library.  Verify the Groth16 proof against the verifying
key, with public inputs `envelope` and the cell tuple.

This check establishes: the hints sealed in the envelope map to the
claimed H3 cell.  **The coordinate lies in the cell only if the device
computed the hints honestly** (see Known limits).

### Joint claim

When all four checks pass, the file supports this statement:

> These pixels are the `crop` (turned by `turn`) of an original that
> device `device` signed, together with an envelope whose sealed hints
> map to cell `cell`, at `captured_at` (device time).

The two proofs run on different fields (BLS12-381 for the crop,
BN254 for the location) and never reference each other.  The device
signature over both the fingerprint and the envelope is what binds
them to one capture.

## C2PA integration

The assertion is embedded by:

1. Constructing a C2PA manifest definition with `c2pa.actions` (`c2pa.created` with `digitalCapture` source type, `c2pa.cropped`, and `c2pa.orientation` when `turn` is non-zero).
2. Adding the assertion under label `io.github.c2pa-thesis.zkloc` via `c2pa::Builder::add_assertion`.  The c2pa-rs library serializes it to CBOR.
3. Signing with ES256 (ECDSA P-256) using the editor's certificate and key.

The editor certificate is separate from the device key.  The device
key signs the receipt; the editor key signs the C2PA manifest.

### Standard tooling behavior

| Validator | Result | Notes |
|-----------|--------|-------|
| `c2pa-rs` (Reader) | `Valid` | Parses and validates the claim signature.  Deserializes the custom assertion. |
| `c2patool` 0.27.16 | `validation_state: Valid` | One informational issue: `signingCredential.untrusted` (expected, sample certs). |
| verify.contentauthenticity.org | `Unrecognized` | Manifest parsed; certificate not on trust list.  **Custom assertion not displayed** — Adobe renders only assertions it knows. |

The `signingCredential.untrusted` status is expected: the pipeline
signs with the c2pa-rs SDK sample certificates, which are not on any
trust list.  This is a property of the test setup, not a defect.

The custom assertion being invisible to third-party verifiers is a
consequence of the C2PA extension model: validators accept unknown
assertions without error but do not render them.  A reader that wants
to see the location claim needs a verifier that knows this spec.

## Version history

| Version | Changes |
|---------|---------|
| 3       | First stable schema on main: fixed 1024×512 original, crop only. |
| 4       | Trigonometric hints added to the envelope. |
| 5       | Original size up to 2^19 pixels (any aspect ratio); crop proof bound to its full statement. |
| 6       | `turn` field: quarter turns clockwise after the crop. |

The reader accepts versions 5 and 6.  A version 5 assertion must have
`turn` absent or 0.

## Known limits

These are properties of the protocol, not of the schema.

- **Location proof holds for an honest prover only.** The zk-Location
  circuit's trigonometric hint outputs are not constrained back to the
  coordinate.  A malicious prover can produce a valid proof for the
  wrong cell (finding A1 in the security analysis).
- **The original is not hidden.** The fingerprint is deterministic, and
  the PST proof carries unmasked evaluations of the original's channel
  polynomials (finding A4).
- **Crop challenges do not depend on the published image** (finding A2)
  and **range-check challenges are drawn before H is committed**
  (finding A3).  A third party can replace the published crop and
  re-sign the manifest.
- **Cell resolution is not bounded** (finding A5).
- **Only the exact coordinate is hidden.** The published cell, time,
  and device key id can trace where the photographer went.
- **The envelope's hiding is assumed, not analyzed.** It is MiMC over
  the coordinate, trigonometric hints and a fresh salt; none of the
  source papers analyzes it as a hiding commitment.
- **Both setups are single-party.** Whoever runs setup could keep the
  trapdoors (finding B3).
- **The device key is a stand-in.** Nothing attests that the key is in
  hardware (finding B1).
- **The editor signs with a public sample key** (finding B2).

## Exit codes

When `provenance verify` rejects a file, the exit code identifies
which check failed:

| Exit code | Failing check |
|-----------|---------------|
| 10        | C2PA manifest |
| 11        | Device receipt |
| 12        | Crop proof |
| 13        | Location proof |

## References

- C2PA Technical Specification 2.2, §6.2.1 (entity-specific namespaces).
- VerITAS (Datta, Chen, Boneh, IEEE S&P 2025): lattice fingerprint and crop proof.
- HyperVerITAS (Greiner, Mowery, Soni, PoPETs 2026): PST instantiation.
- ZKLP (Ernstberger et al., IEEE S&P 2025): zero-knowledge location proof.
- Groth16 (Groth, EUROCRYPT 2016): the location proof system.
- MiMC (Albrecht et al., ASIACRYPT 2016): the envelope commitment.
- H3 (Uber): hexagonal geospatial indexing.
- Security analysis: `docs/security/security-analysis.pdf` (findings A1–A5, B1–B3).
