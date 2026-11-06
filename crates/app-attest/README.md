# app-attest

Verifies an Apple App Attest attestation object, the one
`DCAppAttestService.attestKey` returns, against Apple's App Attest root.
`provenance serve --app-id` uses it when the iPhone app pairs.

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

## What it does not check

- **Fraud risk.** The `receipt` is for "a server-to-server call to request a
  fraud assessment metric from Apple" ([1], Store the public key and
  receipt). Nothing here makes that call or verifies the receipt.
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
- `crates/provenance` pairs with forged attestations: attested, refused for
  another token, key or root (the token is kept), required and missing, a
  software key with one, and no App ID.

CI: `.github/workflows/app-attest.yml`.

Not verified yet: an attestation made by our app on an iPhone. When one is
made, it belongs next to Apple's sample as a second fixture.

## References

1. Apple. [Validating apps that connect to your server](https://developer.apple.com/documentation/devicecheck/validating-apps-that-connect-to-your-server).
2. Apple. [Establishing your app's integrity](https://developer.apple.com/documentation/devicecheck/establishing-your-app-s-integrity).
3. Apple. [Attestation Object Validation Guide](https://developer.apple.com/documentation/devicecheck/attestation-object-validation-guide).
4. Apple. [Preparing to use the app attest service](https://developer.apple.com/documentation/devicecheck/preparing-to-use-the-app-attest-service).
