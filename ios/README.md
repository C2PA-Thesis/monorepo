# ProvenanceCapture

The native iOS capture app. It runs the same flow as the capture page in
`web/` (pair, take a photo, choose the crop, choose the cell, sign and send,
follow the laptop), with three differences:

- The device key lives in the Secure Enclave and every signature asks for
  Face ID, Touch ID or the passcode. The app declares this when it pairs;
  without App Attest nothing proves it to the laptop or to readers, so the
  laptop shows the kind as "declared: secure-enclave".
- The location proof is made on the phone. The coordinate and the salt never
  leave it; the laptop receives the proof, not the secrets.
- After pairing, every request goes over TLS pinned to the laptop's
  certificate key.

SwiftUI, iOS 17 or later, Swift 6 language mode with complete concurrency
checking. Bundle id `edu.utdt.td8.capture`. The project format
(`objectVersion = 77`, synchronized folders) needs Xcode 16 or later. It was
made and tested with Xcode 27.2 only. CI takes the newest Xcode release on the
runner and stops if it is older than 16.

## Layout

```
ProvenanceCapture.xcodeproj   hand-written, folders are synchronized groups
Config/Info.plist             usage strings, ATS local networking
ProvenanceCapture/
  App/                        the app entry and CaptureFlow, the step-by-step model
  Model/                      receipt bytes, crop math, framing, wire types, timeline
  Crypto/DeviceKey.swift      Secure Enclave key, software key, Keychain
  Network/                    PinnedSession, the laptop API, proof parameter download
  Native/NativeBridge.swift   the only file that calls the Rust and Go libraries
  Device/                     camera, QR scanner, CoreLocation
  Views/                      the screens
  Resources/                  asset catalog, sample.jpg (the C2PA sample photo)
ProvenanceCaptureTests/       XCTest unit tests and fixtures
```

New files dropped into these folders are part of the target without editing
the project.

## Simulator and device

| | Simulator | Device |
|---|---|---|
| Device key | software P-256 key in the Keychain (`kind: software`) | Secure Enclave, `.privateKeyUsage` and `.userPresence` (`kind: secure-enclave`, declared by the app, not attested) |
| Pairing | fetches the payload from `http://127.0.0.1:8791/api/v2/pairing`, or paste it | scans the QR code on the presenter page, or paste it |
| Photo | bundled `sample.jpg`, or the photo library | the app's own camera session, no photo library |
| Location | Xcode's simulated location or a GPX file, sent with `simulated: true` | GPS; a fix that iOS marks as simulated by software is refused |

A capture needs a fix taken at most 10 seconds before or after the shutter,
with precise location on. The time is read when the shutter fires (on the
Simulator, when the photo reaches the app). If no such fix arrives by 10
seconds after the shutter, or location is blocked, the capture fails with a
message and the user takes the photo again. Precise location is asked for
again each time the camera opens, because the temporary grant ends with the
app session.

## Running it

On the Mac, once: `scripts/setup.sh` from the repository root, then build the
native libraries (see [Native libraries](#native-libraries)). Each time:
`scripts/demo.sh phone` (the same as `scripts/serve.sh`), or `provenance serve`
by hand. It listens on `127.0.0.1:8791` and, for the app, with TLS on port
8792 of every LAN address, and prints those addresses. If macOS asks whether
`provenance` may accept incoming connections, allow it, or a phone cannot
reach port 8792.

### In the Simulator

Open `ios/ProvenanceCapture.xcodeproj`, pick an iPhone Simulator, run. No
team is needed: Simulator builds sign ad hoc. The first step pairs with the
Mac by itself. Set a location under Features, Location (or Debug, Simulate
Location) before taking the photo, then use the sample photo or pick one.

On 2026-10-30 this ran end to end against `scripts/demo.sh phone`: pairing,
parameter download, cell and location proof on the phone, upload to `POST
/api/v2/captures`, publish on the laptop, and the reader checks. The capture
directory on the laptop held `location_proof.json` and neither the coordinate
nor the salt.

### On an iPhone

Not run yet; these are the steps a free Personal Team needs.

1. In Xcode, Settings, Accounts, add your Apple ID. Xcode lists it as a
   Personal Team. A paid Apple Developer Program team works too, without the
   7-day limit below.
2. Select the ProvenanceCapture target, Signing and Capabilities, and choose
   that team (the project leaves `DEVELOPMENT_TEAM` empty). If Xcode says the
   bundle id `edu.utdt.td8.capture` is not available, change it to one of your
   own; nothing in the code or on the laptop depends on it.
3. On the iPhone, set a passcode if it has none: the Secure Enclave key is
   created with `.userPresence`, and every signature asks for Face ID, Touch
   ID or the passcode.
4. Connect the iPhone by cable (or pair it in Xcode, Window, Devices and
   Simulators), pick it as the run destination and run. iOS asks for
   Developer Mode the first time: Settings, Privacy and Security,
   Developer Mode, turn it on, and restart when asked.
5. The first launch of an app signed by a Personal Team is refused until you
   trust its developer: Settings, General, VPN and Device Management, the
   entry under Developer App for your Apple ID, Trust.
6. Put the iPhone on the same Wi-Fi as the Mac. Networks that isolate clients
   from each other (many guest and campus networks) block it; a phone hotspot
   with the Mac joined to it is an alternative. Unverified on such networks.
7. In the app, scan the large QR code on the presenter page, under "With the
   app", and tap Allow when iOS asks for local network access. If it was
   refused, turn it on in Settings, Privacy and Security, Local Network,
   Capture.

A Personal Team's provisioning profile lasts 7 days: "Provisioning profiles
that enable apps to be installed on a device will expire 7 days from
issuance" ([Developer account overview](https://developer.apple.com/support/compare-memberships/),
Enable a personal team in Xcode). After that the app does not open until you
run it from Xcode again. Running it over the installed app should keep its
key and pairing (unverified); if the app was deleted, pair again and the new
key is trusted next to the old one.

After pairing the app downloads `proving.key` and `circuit.r1cs` from
`/api/params/location/` and checks each SHA-256 against the pairing payload.
They are kept in Application Support (`params/location/`, excluded from
backups) next to a `version` file holding `sealed-hints-v1`, which
`location-proof` checks. The pairing (address, pin, device id, parameter
hashes) is kept in UserDefaults; "Pair again" forgets it and keeps the key.
When the download fails, the step offers both "Download the parameters
again" and "Pair again", because a retry fails for good when the laptop's
parameters, certificate or address changed.

Pairing tries every address in the payload, one at a time, with a 6 second
request timeout and no waiting for connectivity. It repeats the round every 2
seconds for up to 90 seconds on a device (10 in the Simulator), which covers
the local network prompt. When iOS refuses a request because Local Network
access is off, the step says so and names the setting. Requests are never
sent in parallel, because the token is single use. For the same reason each
address is first asked `GET /api/devices/{id}`, which carries no token, and
the pairing request goes only to an address that answered. It is sent again
only when it never connected. When it connected and then timed out or
dropped, the laptop may have spent the token: the app asks the same address
whether the key is now trusted, and if not, asks for a new QR code. In the
Simulator, `https://localhost:8792` is tried first, so pairing works when the
Mac has no LAN address.

After pairing the app keeps one pinned `URLSession`. A request goes to the
address that last answered and moves on to the other announced addresses
when the connection fails. A POST moves on only when no connection was made,
so an upload is never sent twice. No request waits for connectivity, so a
missing network path fails at once instead of after the 600 second resource
timeout.

While it follows a capture, the app polls `GET /api/captures/{id}`. It shows
an error and stops when the answer cannot be read, when the laptop has not
answered for 60 seconds, or when the capture is still running after 15
minutes.

## Pinning

The laptop's certificate is self-signed, so the app does not evaluate the
chain or the host name. `PinDelegate` takes the leaf certificate with
`SecTrustCopyCertificateChain` and its key with `SecCertificateCopyKey`
(`SecTrustCopyKey` may return nothing for a trust that was not evaluated),
rebuilds its SubjectPublicKeyInfo (the fixed P-256 SPKI header followed by the
65-byte point that `SecKeyCopyExternalRepresentation` returns, which is not
SPKI by itself), hashes it with SHA-256 and compares the hex with `pin` from
the payload. Anything else cancels the connection.

## Native libraries

The fingerprint (Rust, `crates/fingerprint-ffi`) and the location circuit (Go,
`location-proof/mobile`) are static XCFrameworks that the app target links
with "Do Not Embed" (they are in its Frameworks build phase, not in an embed
phase). They are build outputs, about 17 MB (Fingerprint) and 26 MB (ZKLoc)
per slice, so they are not committed. Build them before the first build in Xcode:

```
ios/scripts/build-go.sh     # ios/Frameworks/ZKLoc.xcframework, needs Go
ios/scripts/build-rust.sh   # ios/Frameworks/Fingerprint.xcframework, needs rustup
```

Without them the build stops with "There is no XCFramework found at ...". CI
runs both scripts before it builds (`.github/workflows/ios.yml`), so the
Simulator build, the device build and the tests always use the real
libraries. This was chosen over linking them only when present: Xcode has no
build setting that depends on a file existing, and a CI build without them
would compile and test none of the native calls.

Each XCFramework carries its own module map in a folder named after the
module (`Headers/ZKLocFFI/module.modulemap`), so Swift imports `ZKLocFFI` and
`FingerprintFFI` with no search path settings. `NativeBridge.swift` is the
only file that calls them; it still guards each import with `#if canImport`,
so a copy of the project without the libraries compiles and signing stops
with "the native library ... is not built into this app".

The cell is computed on the phone with `zkloc_cell` only. The app never sends
the coordinate to the laptop, so it does not use `POST /api/cell`. Without
ZKLoc no cell is found and Sign and send stays disabled. The library has no
cell outline, so the outline comes from `GET /api/cells/{cell}`, and the app
asks for it only for the published cell, on the result card. While the slider
moves, the map shows the phone's position at the size of an average cell and
nothing goes to the laptop, so the laptop learns no cell finer than the
published one.

## Wire format

The app follows the v2 formats of the shared spec: pairing payload, `POST
/api/v2/pair`, `POST /api/v2/captures` with `location_proof` and
`location: {simulated, accessory}` and no secrets. The receipt's signed bytes
are written by hand in `Receipt.signedBytes`, in the field order of
`crates/provenance/src/receipt.rs`, never with `JSONEncoder`, whose key order
is not specified.

If `/api/v2/pairing` answers 404, the Simulator falls back to
`/api/v2/session`, an older Local route that wraps the same payload as
`{"qr", "expires_in"}`, and takes its `qr` field.

The envelope commits to the coordinate, the salt and the eight trigonometric
hints, which `zkloc_hints` computes on the phone with the same Go code as
`location-proof hints`. The app sends no secrets: `POST /api/v2/captures`
carries the original, the receipt, the cell, the crop, the proof and the
location source, and the laptop verifies the proof against the receipt's
envelope before it stores the capture.

## Tests

```
xcodebuild test -project ProvenanceCapture.xcodeproj -scheme ProvenanceCapture \
  -destination 'platform=iOS Simulator,name=iPhone 17'
```

Any installed iPhone Simulator works; `xcrun simctl list devices available`
lists them.

- The receipt's signed bytes equal, byte for byte,
  `Fixtures/receipt-signed-bytes.json`, which is `JSON.stringify` of the same
  values as `web/src/receipt.ts` builds them.
- The SPKI and pin of a fixed P-256 key equal what OpenSSL computes, both from
  a CryptoKit key and from a `SecKey`. The pin of a self-signed certificate
  made by OpenSSL reads from an unevaluated `SecTrust`, and still reads after
  evaluation fails.
- A CryptoKit DER signature verifies with CryptoKit, and a signed receipt
  verifies over its signed bytes.
- Crop drags and presets, the 2:1 framing window, the timestamp format, the
  pairing payload checks, the upload JSON, the hints in the secrets JSON, the
  timeline rules.
- Rendering the sample photo gives an opaque 1024x512 bitmap whose PNG decodes
  back to the same bytes.
- `NativeTests` run the linked libraries through `NativeBridge`: the Rust
  fingerprint of `fingerprint_test_image` hashes to the digest pinned in
  `crates/fingerprint/src/lib.rs`, and `zkloc_hints`, `zkloc_envelope` and
  `zkloc_cell` on the UTDT coordinate with salt 1 give the hints, envelope
  and resolution 7 cell pinned in `location-proof/main_test.go`. Hints of
  another coordinate are refused.

CI (`.github/workflows/ios.yml`) builds for the Simulator and for a device
without signing, then runs the tests on the first iPhone Simulator of the
runner.

## Not verified yet

- Anything on a device: the Secure Enclave key, the camera, the QR scanner,
  the local network prompt and the retry after it, pinned TLS to a LAN
  address under ATS with `NSAllowsLocalNetworking`, the device setup steps
  above. The device build compiles (CI builds it unsigned).
- `zkloc_prove` on a device: its time and memory there. In the Simulator it
  runs in the tests when they get params
  (`TEST_RUNNER_ZKLOC_PARAMS=<dir from location-proof setup>`, and
  `TEST_RUNNER_ZKLOC_PROOF_OUT=<file>` to keep the proof), and in the end to
  end run above; CI skips it. On the laptop, `location-proof prove` peaked at
  a 75 MB memory footprint and took 0.33 s (`/usr/bin/time -l`, 2026-10-30).
- The end to end flow on a device. In the Simulator it ran on 2026-10-30.
