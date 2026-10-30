# Capture and presenter pages

Two pages, served by `provenance serve --web web/dist`.

- **Capture page** (`index.html`, at `/`). What a phone opens. It takes the
  photo, reads the GPS fix, computes the fingerprint and the envelope itself,
  signs the receipt with a key that never leaves the browser, and uploads the
  capture. Nothing leaves the phone unsigned. It then follows the laptop as
  it proves, signs and verifies, and ends on the verified file.
- **Presenter page** (`screen.html`, at `/screen`). What the laptop shows. A
  QR code that opens the capture page and pairs the phone, every capture with
  its private original next to its published crop, the proofs and reader
  checks as they run, the cell on a map, and a tab that verifies any dropped
  PNG. Its calls only answer on the laptop itself.

## Run

Requires Node 22 and the Rust toolchain with the `wasm32-unknown-unknown`
target (`rustup target add wasm32-unknown-unknown`). From this directory:

```bash
npm install
npm run wasm     # builds crates/fingerprint for the browser into public/
npm run build    # type-checks and writes dist/
```

Then, from the repository root, `scripts/demo.sh phone` serves both pages and
the API, exposes them to the phone over HTTPS through ngrok, and opens the
presenter page. Browsers only allow the camera and geolocation on a secure
origin, so plain HTTP over the LAN will not do. By hand:

```bash
ngrok http 8791                                          # note its https URL
provenance serve --web web/dist --public-url https://…   # in another terminal
```

Open `http://127.0.0.1:8791/screen` on the laptop and scan its QR code with
the phone.

For page development, `npm run dev` serves the sources with hot reload and
proxies `/api` to a local `provenance serve`.

## What the page does

| Step | Where | Notes |
| --- | --- | --- |
| Device key | `src/key.ts` | Non-extractable P-256 key in IndexedDB, created once. Its id is the SHA-256 of the SPKI DER, as the receipt names it. Pairing sends the public key in PEM with the code the laptop printed |
| Photo | `src/camera.ts` | The native camera through a file input. A 2:1 window the photographer slides, default centered, resized to 1024x512 in a canvas. The pixels and the PNG come from the same canvas |
| Crop | `src/crop.ts` | The rectangle of that original to publish, drawn or moved by dragging on the preview, or picked from presets. Default the left half. Recorded with the capture; publishing applies and proves it |
| GPS | `src/main.ts` | Watched from page load; the fix and the phone clock are sampled when the photo reaches the page, a few seconds after the shutter |
| Cell | `src/api.ts`, `src/map.ts` | Asked from `POST /api/cell` for the chosen resolution, and drawn on an OpenStreetMap base map from `GET /api/cells/{cell}`. The page never maps coordinates itself |
| Fingerprint | `src/fingerprint.ts` | `crates/fingerprint` built for wasm, 128 rows split across web workers. 12.8 s on an iPhone with four workers |
| Envelope | `src/envelope.ts` | MiMC over BN254 as gnark-crypto computes it, checked against vectors from `location-proof commit` |
| Receipt | `src/receipt.ts` | The compact JSON `crates/provenance` signs, in its field order, signed with WebCrypto and DER-encoded |
| Progress | `src/timeline.ts` | The publish stages and reader checks the laptop reports, polled from `GET /api/captures/{id}`. Shared with the presenter page |

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
- The map tiles come from tile.openstreetmap.org, so the phone and the
  laptop fetch tiles around the cell from it.
- The frame slider moves the window along one axis. A photo already at 2:1
  has nothing to slide, so the slider is hidden.
