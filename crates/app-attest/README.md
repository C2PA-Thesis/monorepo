# app-attest

Verifies an Apple App Attest attestation object, the one
`DCAppAttestService.attestKey` returns, against Apple's App Attest root, and
the App Attest receipt that carries Apple's risk metric. Also signs the token
for Apple's `attestationData` endpoint. `provenance serve --app-id` uses it
when the iPhone app pairs, and `--open-registration` when it registers.

```rust
let attestation = app_attest::verify(
    &object,
    &app_attest::Expected { app_id, key_id: &key_id, client_data_hash: &hash },
    now_unix_seconds,
)?;
```

## What the app attests

App Attest certifies a key of its own, which the device "automatically stores
[...] in the Secure Enclave" ([2], Create a key pair). That key is not the
P-256 key that signs the app's receipts. At pairing the app passes
`clientDataHash = SHA256("io.github.c2pa-thesis.pair.v2\n" + token + "\n" +
device_id)`, where `token` is the single-use token from the QR code and
`device_id` the SHA-256 of the receipt key's SubjectPublicKeyInfo. The laptop
builds the same bytes from the token it issued and the public key in the same
request (`pairing_client_data` in `crates/provenance/src/serve.rs`,
`PairingAttestation.clientData` in the app; a test on each side pins the same
hash). The token is 16 random bytes, as Apple asks: "The challenge should be
at least 16 bytes in length" ([2], Certify the key pairs as valid).

To register without a QR code the app passes
`SHA256("io.github.c2pa-thesis.register.v1\n" + challenge + "\n" + device_id)`,
where `challenge` is 32 random bytes in base64 from `POST /api/v2/challenge`
(`registration_client_data` in `crates/provenance/src/register.rs`,
`RegistrationAttestation.clientData` in the app, pinned on both sides). The
prefixes differ, so an attestation made for one route fails the nonce on the
other.

## What `verify` checks

The steps of [1], Verify the attestation, in order:

1. The CBOR object has `fmt: "apple-appattest"`, `attStmt.x5c`,
   `attStmt.receipt` and `authData`, with no trailing bytes.
2. Each certificate in `x5c` is valid at `now`, names the next one (or the
   root) as issuer, and its signature verifies under that issuer's key, which
   must be a CA. The root is Apple App Attestation Root CA, bundled in
   `src/` with its SHA-256 pinned in a test
   (`1cb9823b...6242c932`, the PEM that apple.com served on 2026-11-05).
3. `nonce = SHA256(authData || clientDataHash)` equals the octet string in
   the credential certificate's extension `1.2.840.113635.100.8.2`.
4. SHA-256 of the credential certificate's key, as an uncompressed X9.62
   point, equals the key id.
5. The RP ID equals SHA-256 of the App ID: "the concatenation of your app's
   App ID prefix, a period, and your app's CFBundleIdentifier value" ([1]).
6. The counter is 0.
7. The aaguid is `appattestdevelop` (development) or `appattest` followed by
   seven zero bytes (production).
8. The credential id equals the key id.
9. Not one of Apple's steps: the COSE key in `authData` is the certificate's key.
10. `apple_validation_category_01`, when present, is not 0 ("Invalid") nor 7,
    8 or 9 ("These values aren't appropriate"), nor above 10, which [1] does
    not list. `apple_bundle_version_01` is recorded, not compared with
    anything.

## Receipts

The `receipt` in an attestation is for "a server-to-server call to request a
fraud assessment metric from Apple" ([1], Store the public key and receipt).
The receipt in the attestation is of type `ATTEST`; Apple's answer is of type
`RECEIPT`, and "Only the latter provide the risk metric in field 17" ([5],
Interpret the metric). `receipt::verify_receipt` follows [5], Verify the
receipt, steps 1 to 3:

1. The PKCS #7 signed data parses, in BER as Apple sends it (indefinite
   lengths, a chunked octet string). The ECDSA P-256 SHA-256 signature
   verifies over the content, or over the signed attributes when there are
   any, with their message digest checked. Apple's sample has none.
2. The signer is the included certificate whose serial the signer info names,
   with the common name "Application Attestation Fraud Receipt Signing", and
   its chain through the included intermediate leads to Apple Root CA - G3 at
   `now`. [5] says to evaluate the signing certificate "up to the" root "for
   App Attest"; the copy of the page we read lost the link's name. Apple's
   sample receipt chains to Apple Root CA - G3 (checked with OpenSSL), not to
   the App Attestation root, so the crate pins G3, SHA-256 `63343abf...653e9179`.
3. The payload is read: fields 2 (App ID), 3 (attested certificate), 4, 5, 6
   (type), 7, 12 (creation), 17 (risk metric), 19 and 21. Field 7 is not in
   Apple's table; the sample has `production` there.

Steps 4 to 6 (App ID, at most five minutes old, the attested key) are the
caller's, with the fields `Receipt` returns: `crates/provenance` does them at
registration. `jwt::DeviceCheckKey` makes the token: "Generate the token using
the same procedure you use to create a provider authentication token for the
Apple Push Notification service (APNs)" ([5], Create a request), so ES256 with
the key id in the header and the Team ID and issue time in the claims.

## What it does not check

- **Fraud risk at pairing.** Only registration asks Apple for the metric. The
  server asks once and does not refresh it before field 21 expires.
- **Where the receipt key lives.** The attestation is over App Attest's key.
  That the receipt key is in the Secure Enclave follows from the app's code
  (`SecureEnclaveDeviceKey`), not from anything the laptop can check.
- **Which source code was built.** App Attest certifies the App ID and the
  code signature. Any build signed by the team attests the same, modified or
  not. A build from Xcode has category 3, "An executable signed by a
  development code signing identity" ([1]).
- **Revocation** of the certificates.
- **Assertions.** The app attests once per pairing and signs nothing with
  App Attest afterwards; captures are signed by the receipt key only.
- **Readers.** The attestation stays on the laptop, in the device's sidecar.
  The published file carries none of it.

## Two inconsistencies in Apple's documentation

- The development aaguid. [1] and [3] say `appattestdevelop`; [4] says
  "expect to find the value `appattestsandbox`". This crate accepts
  `appattestdevelop` only. Not verified yet on a device.
- [3] gives the "Expected public key SHA256 hash" as `inGjK2Jb...`, which is
  not the key id `zgSY9YSD...`. The SHA-256 of the uncompressed point, which
  step 4 asks for, computed with OpenSSL, is `zgSY9YSD...`, so this crate
  follows the step.

Apple's sample also differs from the guide's text in two places, and the
code follows the sample: its `authData` carries the extensions map without
the extensions flag (`0x80`), and its bundle version is `"1"` where the text
says `"1.0"`. The sample's composite before the nonce ends with the raw bytes
of `example_server_challenge`, so the sample app passed the challenge itself
as `clientDataHash`, not its hash.

## Tests

```
cargo test -p app-attest --all-features
```

- `tests/apple_sample.rs`: Apple's sample attestation object from [3]
  (Team ID `1234567890`, `com.example.myapp`). Its credential certificate is
  valid from 2026-04-20 18:13:12 to 2026-04-23 18:13:12 UTC, so it verifies
  at 2026-04-21 and is refused at 2026-11-05. Also refused: another App ID,
  another challenge, another key id, a truncated or padded object.
- `src/tests.rs`: attestations forged by `testing::Forge` under a test root,
  each failing one check (root, expiry, nonce, client data, App ID, counter,
  aaguid, credential id, category, altered authData, fmt, short chain).
- `tests/apple_sample.rs` also verifies the sample's `ATTEST` receipt at
  2026-04-21 (type, App ID, environment, creation 2026-04-21T18:13:12Z, the
  attested key), and refuses it after its signer expires on 2027-02-18, under
  the App Attestation root, and with a flipped byte.
- `src/receipt.rs`: receipts forged by `testing::Forge`, of type `RECEIPT`
  with a metric, the `ATTEST` one inside a forged attestation, and refusals
  for a tampered payload, another signer name, another root and another type.
  BER indefinite lengths and the time format have their own tests.
- `src/jwt.rs`: the token's header, claims and signature, and refused keys
  and ids.
- `crates/provenance` pairs with forged attestations: attested, refused for
  another token, key or root (the token is kept), required and missing, a
  software key with one, and no App ID. It registers with them against a fake
  Apple that answers with forged receipts: the metric recorded, the challenge
  spent by a refused attempt, a pairing attestation or another key refused,
  the category policy, an App Attest key already used, a metric above the
  ceiling, a stale receipt, one for another App ID, one under another root.

CI: `.github/workflows/app-attest.yml`.

Not verified yet: an attestation made by our app on an iPhone, and a
`RECEIPT` from Apple's server. When there are, they belong next to Apple's
sample as fixtures. Whether a development receipt chains to the same root is
not known.

## References

1. Apple. [Validating apps that connect to your server](https://developer.apple.com/documentation/devicecheck/validating-apps-that-connect-to-your-server).
2. Apple. [Establishing your app's integrity](https://developer.apple.com/documentation/devicecheck/establishing-your-app-s-integrity).
3. Apple. [Attestation Object Validation Guide](https://developer.apple.com/documentation/devicecheck/attestation-object-validation-guide).
4. Apple. [Preparing to use the app attest service](https://developer.apple.com/documentation/devicecheck/preparing-to-use-the-app-attest-service).
5. Apple. [Assessing fraud risk](https://developer.apple.com/documentation/devicecheck/assessing-fraud-risk).
