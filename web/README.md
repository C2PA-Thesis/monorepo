# Capture and presenter pages

Two pages, served by `provenance serve --web web/dist`.

- **Capture page** (`index.html`, at `/`). What a phone's browser opens. It
  takes the photo, reads the GPS fix, computes the fingerprint, the
  trigonometric hints and the envelope itself, signs the receipt with a key
  that never leaves the browser, and uploads the capture with the coordinate,
  the salt, the hints, the crop rectangle and the turn. The laptop then makes both proofs. The page follows
  it as it proves, signs and verifies, and ends on the verified file.
- **Presenter page** (`screen.html`, at `/screen`). What the laptop shows. A
  large QR code that pairs the iOS app and a smaller one that opens the
  capture page and pairs the browser, every capture with its private original
  next to its published crop, the proofs and reader checks as they run, the
  cell on a map, and a tab that verifies any dropped PNG. Its calls only
  answer on the laptop itself.

The native app in [`ios/`](../ios/README.md) is the recommended way to
capture: it keeps the coordinate and the salt on the phone and sends the
laptop a location proof instead. The capture page remains for phones without
the app.

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

Open `http://127.0.0.1:8791/screen` on the laptop, choose "Use a browser
instead" under the QR code, and scan the browser QR code with the phone's
camera.

For page development, `npm run dev` serves the sources with hot reload and
proxies `/api` to a local `provenance serve`.

## What the page does

| Step | Where | Notes |
| --- | --- | --- |
| Device key | `src/key.ts` | Non-extractable P-256 key in IndexedDB, created once. Its id is the SHA-256 of the SPKI DER, as the receipt names it. Pairing sends the public key in PEM with the code the laptop printed |
| Photo | `src/camera.ts` | The native camera through a file input. The whole photo, scaled down in a canvas to at most 2^19 pixels with its aspect ratio kept (`fit` in `src/fingerprint.ts`, the same as `Size::fit` in `crates/fingerprint`). The pixels and the PNG come from the same canvas |
| Crop | `src/crop.ts` | The rectangle of that original to publish, drawn or moved by dragging on the preview, or picked from presets. Default the left half. Recorded with the capture; publishing applies and proves it |
| Turn | `src/crop.ts` | Quarter turns clockwise applied to the cropped pixels, 0 to 3, cycled by the Rotate chip. The preview shows the whole frame turned, and the presets name halves of that turned frame, but the rectangle sent stays in the original's pixels. Sent as `turn` with the upload; publishing turns the crop and the crop proof covers the turn. For 90 and 270 degrees the published image is the rectangle's height by its width |
| GPS | `src/main.ts` | Watched from page load; the fix and the phone clock are sampled when the photo reaches the page, a few seconds after the shutter |
| Cell | `src/api.ts`, `src/map.ts` | Asked from `POST /api/cell` for the chosen resolution, and drawn on an OpenStreetMap base map from `GET /api/cells/{cell}`. The page never maps coordinates itself |
| Fingerprint | `src/fingerprint.ts` | `crates/fingerprint` built for wasm, 128 rows split across web workers. 12.8 s on an iPhone with four workers |
| Hints | `src/envelope.ts` | The eight float32 bit patterns the location circuit takes: for the latitude, then the longitude, tan, 2 sin cos, sin and cos of half the float32 radians, each computed in float64 (`Math.tan`, `Math.sin`, `Math.cos`) and rounded once with `Math.fround`, as Go's `NativeHints` does |
| Envelope | `src/envelope.ts` | MiMC over BN254 of the coordinate bits, the salt and the hints, in that order, as gnark-crypto computes it. Sealing the hints means the laptop cannot pick other hints to prove the envelope into a cell that does not contain the point they give |
| Receipt | `src/receipt.ts` | The compact JSON `crates/provenance` signs, in its field order, signed with WebCrypto and DER-encoded |
| Progress | `src/timeline.ts` | The publish stages and reader checks the laptop reports, polled from `GET /api/captures/{id}`. Shared with the presenter page |

## Tests

```bash
npm test
```

The turn helpers against the pixel mapping of `Turn::source` in
`crates/crop-proof` (the box on the turned preview holds the pixels that
cropping then turning gives), the hints and the envelope against nine vectors from `location-proof hints`
and `location-proof commit` (including the poles and the antimeridian), a
changed hint giving another envelope, the receipt bytes against the Rust field
order, DER encoding, and the wasm fingerprint of the built-in test image
against the digest pinned in `crates/fingerprint`. `npm run wasm`
must have run first. The `web` workflow runs all of it.

## Limits

- The coordinate, the salt and the hints leave the phone. The page sends them
  to the laptop in `secrets` with the upload, through the ngrok tunnel (TLS to
  ngrok, then ngrok to the laptop), because the laptop makes the location
  proof. The laptop stores them in `secrets.json`. The app does not send
  them.
- The cell is only as trustworthy as this page's code: the circuit checks the
  hints against the signed envelope, not against the coordinate (see Known
  limits in the main README). On this path the laptop's `location-proof` also
  refuses hints that do not match the coordinate, but the page chooses the
  coordinate it signs.
- The receipt signs neither the cell nor the resolution. The laptop holds
  `secrets.json`, so it can publish the same envelope with any cell that
  contains the point, from resolution 4 to 9, finer than the one chosen on the
  phone (see Known limits in the main README).
- The key is a stand-in for a hardware key. Safari can evict site storage
  after a week without use; if the key is gone, pair again and the new key is
  trusted alongside the old one.
- The coordinate and time are taken when the photo returns to the page, not
  at the shutter.
- The map tiles come from tile.openstreetmap.org, so the phone and the
  laptop fetch tiles around the cell from it.
