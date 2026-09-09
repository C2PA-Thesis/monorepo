# Provenance Lab

A local visual interface for the current C2PA thesis pipeline. Upload an image,
choose a simulated capture location, preview the fixed left crop, and follow
the actual HyperVerITAS, ZKLP, and C2PA commands through publication and verification.

## Run

From the repository root:

```bash
./web/run.sh
```

Open **http://127.0.0.1:8042**. The first launch prepares the existing pipeline,
installs the web dependencies, and builds the page. You need the pipeline's
Git, Go, C compiler, Rust nightly, Python, and authenticated GitHub CLI setup,
plus Node.js 22. See [the pipeline guide](../pipeline/README.md).

After setup and a web build, reuse the installed tools:

```bash
WEB_SKIP_SETUP=1 ./web/run.sh
```

Use `LAB_PORT=8043` to change the port. Stop the server with Ctrl-C. It stops
active proof processes and retains their files. Restarted incomplete operations
are marked interrupted, and completed experiments remain available.

For frontend development, run the backend above in one terminal, then
`npm --prefix web run dev` in another. Open the Vite URL. Vite proxies `/api`
to port 8042. A production build uses `npm --prefix web run build`.

## Explore

1. **Experiment:** upload a PNG, JPEG, or WebP, or use the C2PA SDK sample.
   The lab converts it to 1024 × 512 RGB, potentially changing its aspect ratio.
   Select a map point, enter coordinates, or use a location preset. The slider
   selects H3 resolution. The actual ZKLP CLI checks whether the cell is supported.
2. **Crop:** inspect the original, the highlighted 512 × 512 left half, and the
   before/after slider. The crop boundary is fixed. No other edit is supported.
3. **Generate real proofs:** freeze the selected image and location. Eight stages
   run in order, with stage timings, live English messages, and full command logs.
   You can explore other tabs while the prover runs. Stop cancels the process group.
4. **Inside the pixels:** move an 8 × 8 patch over the actual normalized image,
   inspect RGB values and row-major vector indices, and work through a small
   matrix example. The modulo-97 example is explicitly a teaching model. The
   128 fingerprint elements per channel shown after capture are actual values.
5. **Evidence:** inspect the signed receipt, public region, proofs, and manifest
   decoded from the actual signed PNG. Download the PNG or full manifest. Verify
   again invokes the existing reader verifier on the published asset.
6. **Try to break it:** alter a pixel, forge a device signature, request another
   region, or swap a receipt from a fresh capture. Each case creates a separate
   C2PA-signed asset and requires the expected custom check to reject it. The
   original proof bundle remains available. All six automatic negative cases
   also run as the last pipeline stage.

**Reader view** hides the original image, exact location, pixel explorer, and
capture logs. It shows public evidence and stage results. This is an educational
perspective within a local application, not an access-control or privacy boundary.

## Structure and data

- `src/`: React interface, Leaflet map, pixel explorer, and evidence inspector.
- `server.py`: local FastAPI API, uploads, real region preflight, persisted jobs,
  one proof worker, replayable server-sent events, cancellation, and artifact routes.
- `attack.py`: independent mutations checked by the real packaged reader.
- `../pipeline/lib/workflow.py`: the eight commands shared by the CLI and web app.
- `generated/runs/<id>/`: isolated original images, private capture inputs, logs,
  public proof artifacts, and saved experiment state. Ignored by Git.

The server listens on loopback and accepts local hosts and origins. It is intended
for one person's machine. Images and proof computation stay on that machine;
OpenStreetMap serves basemap tiles. Fonts are bundled locally. Uploads are limited
to 20 MB and 40 million pixels. The app does not read GPS metadata.

Share the signed PNG, not the experiment directory. The latter contains private
prover inputs. Public artifact routes use a fixed allowlist; private salts and
signing keys are not downloadable through those routes.

## Verification

```bash
PYTHONPATH=pipeline:web pipeline/.venv/bin/python -m unittest discover -s web/tests -v
npm --prefix web run build
pipeline/.venv/bin/python web/tests/integration.py
```

The last command starts `web/run.sh` on an unused port with isolated output,
uploads a real image, selects San Francisco, runs all eight stages, consumes
live events, and checks the signed crop and decoded public evidence. It requires
completed setup and a frontend build. Evidence is retained under
`generated/integration/`. The web workflow runs these checks in CI. Browser
interaction and visual review are additional local checks, not simulated by the
HTTP integration script.

## Research boundaries

The interface retains the [pipeline's security limitations](../pipeline/README.md).
The capture location and device are simulated. The device signature connects two
different public values; the proof systems do not share a field commitment.

The pinned location code rejects H3 pentagons, cells spanning multiple
icosahedron faces, inconsistent native face mappings, and centers that do not
round-trip through its FP32 mapping. These checks come from
[`locproof/zkloc/region.go`](../locproof/zkloc/region.go). A different issue is
the inherited circuit's unconstrained trigonometric hints: passing the intended
prover flow does not establish security against a modified prover.

The deterministic image fingerprint enables candidate matching, and the pinned
image proof exposes unmasked polynomial evaluations. Complete image privacy is
not established. Sample C2PA certificates and local proof parameters are research
identities and setup material, not production trust anchors.
