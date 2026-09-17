# Capture page

The page a phone opens to make a capture. It takes the photo, reads the GPS
fix, computes the fingerprint and the envelope itself, signs the receipt with
a key that never leaves the browser, and uploads the capture to
`provenance serve`. Nothing leaves the phone unsigned.

## Run

Requires Node 22 and the Rust toolchain with the `wasm32-unknown-unknown`
target (`rustup target add wasm32-unknown-unknown`). From this directory:

```bash
npm install
npm run wasm     # builds crates/fingerprint for the browser into public/
npm run build    # type-checks and writes dist/
```

Then, from the repository root, serve the page and the API together and
expose them to the phone over HTTPS. Browsers only allow the camera and
geolocation on a secure origin, so plain HTTP over the LAN will not do.
`scripts/serve.sh` does both and prints the URL; by hand:

```bash
provenance serve --web web/dist    # prints a pairing code
ngrok http 8791                    # in another terminal; open its https URL on the phone
```

On the phone: type the pairing code once, take a photo, slide the frame, drag
the crop, pick a resolution, and tap Sign and upload. The page prints the
capture directory; on the laptop, `provenance publish --capture DIR` proves
and publishes it, and `provenance verify` checks the result.

For page development, `npm run dev` serves the sources with hot reload and
proxies `/api` to a local `provenance serve`.

## What the page does

| Step | Where | Notes |
| --- | --- | --- |
| Device key | `src/key.ts` | Non-extractable P-256 key in IndexedDB, created once. Its id is the SHA-256 of the SPKI DER, as the receipt names it. Pairing sends the public key in PEM with the code the laptop printed |
| Photo | `src/camera.ts` | The native camera through a file input. A 2:1 window the photographer slides, default centered, resized to 1024x512 in a canvas. The pixels and the PNG come from the same canvas |
| Crop | `src/crop.ts` | The rectangle of that original to publish, drawn by dragging on the preview, default the left half. Recorded with the capture; `provenance publish` applies and proves it |
| GPS | `src/main.ts` | Watched from page load; the fix and the phone clock are sampled when the photo reaches the page, a few seconds after the shutter |
| Cell | `src/api.ts` | Asked from `POST /api/cell` for the chosen resolution. The page never maps coordinates itself |
| Fingerprint | `src/fingerprint.ts` | `crates/fingerprint` built for wasm, 128 rows split across web workers. 12.8 s on an iPhone with four workers |
| Envelope | `src/envelope.ts` | MiMC over BN254 as gnark-crypto computes it, checked against vectors from `location-proof commit` |
| Receipt | `src/receipt.ts` | The compact JSON `crates/provenance` signs, in its field order, signed with WebCrypto and DER-encoded |

## Tests

```bash
npm test
```

The envelope against four vectors from the Go tool, the receipt bytes against
the Rust field order, DER encoding, and the wasm fingerprint of the built-in
test image against the digest pinned in `crates/fingerprint`. `npm run wasm`
must have run first. The `web` workflow runs all of it.

## Limits

- The key is a stand-in for a hardware key. Safari can evict site storage
  after a week without use; if the key is gone, pair again and the new key is
  trusted alongside the old one.
- The coordinate and time are taken when the photo returns to the page, not
  at the shutter.
- No map of the cell yet, only its id.
- The frame slider moves the window along one axis. A photo already at 2:1
  has nothing to slide.
