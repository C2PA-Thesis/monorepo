# Local verification, 2026-09-07

These results cover the local web implementation on the working branch. The
GitHub workflow has been added; remote CI has not run for these uncommitted changes.

## Automated checks

- 8 web tests passed, including actual region preflight, invalid uploads,
  isolated files, private-artifact rejection, process failure, cancellation,
  and interrupted-run recovery.
- 24 existing pipeline tests passed after extracting the shared stage plan.
- TypeScript and the Vite production build passed.
- `npm audit --audit-level=moderate` reported zero vulnerabilities.
- Shell syntax and `git diff --check` passed.
- `web/tests/integration.py` passed through the actual `web/run.sh` entry point.
  It received 65 live events, completed all eight stages, compared published
  pixels with the expected crop, decoded the actual signed manifest, and checked
  all six negative cases. Output is retained locally at
  `generated/integration/f1c5f3cf40af44efb3a834f227664e2a/`.

## Real proof runs

| Input and location | Pipeline duration | Result |
| --- | --- | --- |
| C2PA sample, UTDT, resolution 7 | 137.6 seconds | 8 stages and 6 negative cases passed |
| Uploaded PNG, Tokyo, resolution 7 | 136.5 seconds | 8 stages and 6 negative cases passed |
| Uploaded sample through HTTP, San Francisco, resolution 7 | 136.0 seconds | 8 stages, SSE, exact crop, reader evidence, and 6 negative cases passed |

Timings reflect this machine with existing parameters and compiled proof tools.
First-time setup is additional work. They are observations, not progress estimates.

## Browser checks

The local Codex browser exercised uploads, presets, numeric coordinates, a map
click, the resolution slider, fixed crop previews, and restored saved experiments.
Latitude 91 disabled proving with an explanatory error. The resolution-7 H3
pentagon `870800000ffffff` showed the real CLI rejection. A run at resolution 8
was stopped during capture; its process ended and its files were retained.

The pixel explorer showed real RGB values, vector indices, and fingerprint
elements. Changing channels and advancing the teaching matrix row updated the
display. Reader view hid original pixels and exact coordinates. The evidence
inspector displayed the decoded signed manifest with `validation_state: Valid`.
Reverification passed. Manifest download was exercised in the browser, and the
signed PNG download was checked by the HTTP integration test.

All four interactive attacks on the Tokyo bundle were run through the page:
pixel mutation failed check 2, a forged device signature failed check 1, a
neighboring region failed check 3, and a fresh receipt swap failed check 3.
Every mutated package retained C2PA validity.

Desktop and 390-pixel mobile layouts were inspected, including the public
evidence viewer. The mobile document had no horizontal overflow. The browser
reported no application console warnings or errors during those checks.

The real computations do not resolve the security and privacy limitations in
[the lab guide](README.md#research-boundaries).
