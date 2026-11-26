# A2 and A3 against upstream HyperVerITAS

Findings A2 and A3 of the [security analysis](../security-analysis.pdf) are in
the reference implementation too, not only in our extraction in
`crates/crop-proof`. `reproduce.sh` runs both attacks against
[`glgreiner/HyperVerITAS`](https://github.com/glgreiner/HyperVerITAS) at
`915c48a`, with its submodules `hyperplonk` at `bed02ac` and
`plonkish_basefold` at `45a16ea`.

```bash
docs/security/hyperveritas-upstream/reproduce.sh
```

It needs git, python3, a Rust nightly toolchain (upstream's `install.sh` asks
for one; set `HV_TOOLCHAIN` to pick another) and about 7 GB of free memory.
It exits non-zero if any case ends differently from the table below. Upstream is cloned
and built in `$TMPDIR/hyperveritas-upstream` (or the directory given as the
first argument, which must be outside the monorepo), and the logs go to its
`run/` folder.

| Case | What the attacker does | Expected |
| :-- | :-- | :-- |
| `honest` | Proves and verifies the honest crop | accept |
| `a2` | Reuses the honest proof with another published image | accept |
| `a2-control` | Reuses the honest proof with the uncorrected gradient | reject |
| `a3` | Proves from the fingerprint alone that a gradient is the crop | accept |
| `a3-control` | Same forged proof, but the verifier reads the honest crop | reject |

## Where the flaws are

In `hyperveritas_impl/examples/hv_crop_pst.rs` at `915c48a`, the verifier
`ver()` (lines 557 to 936):

1. absorbs the image commitments (line 610);
2. draws the two range-check challenges (lines 665 and 666), but absorbs the
   commitment to the sorted vector H only at line 776. This is A3. The prover's
   `multsetProverIOP` (`src/prover.rs`, lines 99 and 115) draws them before H
   is committed, so a dishonest prover sees them before choosing H;
3. draws the crop challenges (line 672) without ever absorbing the published
   crop or the fingerprint. This is A2.

## What the script changes

- **`verifier-io.patch`** is the only change to `ver()`, and it is input and
  output. `HV_VERIFIER_CROP` makes the verifier read the published crop from
  another file, and `HV_DUMP_CHALLENGES` writes the crop challenges to a file.
  No check changes.
- **`fixtures.py`** writes a random 1024 x 512 original, its left half as the
  honest crop and an unrelated 512 x 512 gradient.
- **A2.** `forge/` adjusts 64 pixels per channel of the gradient with LLL and
  Babai so that the challenge-weighted sum matches the honest crop mod p. It
  is `substitute_crop` from `crates/crop-proof/src/forge.rs` and compiles the
  same solver, `crates/crop-proof/src/lattice.rs`.
- **A3.** `make_a3.py` generates `hv_crop_pst_a3.rs` from the patched example.
  Only the prover changes: it starts from the fingerprint the example's camera
  computes, puts the gradient in the crop and gray elsewhere, and solves 128
  pixels outside the crop so that A I' equals the fingerprint exactly. Its
  range check reads the two challenges ahead and picks an H that is zero except
  at two positions, the second one the root of a quadratic. `ver()` is the
  patched one, unchanged.

## Limits

- Prover and verifier run in the same process, as in the upstream example. The
  verifier receives the proof as arguments of `ver()`, not serialized.
- The example seeds its randomness with `test_rng()`, which is deterministic.
- The fixtures are synthetic. The attacks do not depend on the image, but no
  real photo was used here.
