# Memory hill-climb of the crop prover

Goal: run the crop prover on an iPhone 17 Pro. One row per attempt, kept or not.

- **Reference case**: 1024x512 original (2^19 pixels), 512x512 crop, no turn.
- **Metric**: peak memory footprint of the proving process, which is what
  `/usr/bin/time -l` prints as "peak memory footprint" and what iOS compares
  with the app's limit. `bench probe` reads the same number from the kernel
  right after the proof.
- **Machine**: Apple M5 Max, 18 cores, 128 GB, macOS 26.6.2. All cores unless
  a row says otherwise. The machine was not idle (a VM and a system daemon
  held about one core), so times are a few percent worse than a quiet run.
- **Each number** is the median of 3 runs of `scripts/hillclimb-measure.sh`,
  which also verifies the proof it made. Experiments marked "1 run" ran once.
- **Proof hash**: the prover is deterministic, so for fixed parameters and
  inputs the proof bytes are fixed. `83ba522c65a2c842` is the first 16 hex
  digits of the SHA-256 of the reference proof under the parameters in this
  checkout's `.provenance`. A Tier 0 step must reproduce it.
- **Gates**: G1 `cargo test --workspace`; G2 clippy with attack hooks;
  G3 `cargo test --release -p crop-proof --features attack-hooks -- --ignored`
  (round trip, A1 to A3, and `same_proof`, which compares the proof with the
  reference prover's on three inputs); G4 `provenance setup && demo && attack`;
  G5 `forged_crops`.

## Where the memory went at baseline

`bench probe` with the `mem-profile` feature (a counting allocator and a
footprint sampler), one run:

| Phase | Seconds | Live heap at start | Peak live heap | Peak footprint |
|---|---:|---:|---:|---:|
| load parameters | 1.15 | 2 MB | 422 MB | 428 MB |
| commit image | 0.78 | 221 MB | 456 MB | 808 MB |
| hash weights | 0.25 | 271 MB | 573 MB | 1078 MB |
| hash sumcheck | 0.08 | 288 MB | 405 MB | 1151 MB |
| range check 0 | 2.01 | 271 MB | 1496 MB | 2755 MB |
| range check 1 | 2.30 | 372 MB | 1596 MB | 2463 MB |
| range check 2 | 2.24 | 472 MB | 1697 MB | 2409 MB |
| crop sumcheck | 0.59 | 573 MB | 716 MB | 1464 MB |
| image opening | 0.78 | 582 MB | 1940 MB | 2518 MB |
| range opening | 3.03 | 582 MB | **4910 MB** | **5879 MB** |

The scouts estimated 4.4 to 4.9 GB of live data in the range opening and
could not explain the last gigabyte. The live heap peaks at 4.91 GB. The
other gigabyte is the system allocator's cache of freed large blocks: with
`MallocLargeCache=0` the footprint peak is 4919 MB, the live heap peak
exactly. After the proof the process still holds 2.2 GB of footprint for
0.58 GB of live heap. So the footprint follows the live heap plus up to a
gigabyte of recently freed large blocks, and cutting large allocate/free
cycles pays twice.

## Ledger

| # | Commit | Tier | Idea | Footprint | RSS | Prove | Hash same | Gates | Kept | Lesson |
|---|---|---|---|---:|---:|---:|---|---|---|---|
| 0 | 537379c | - | Baseline (`origin/main` plus the probe) | 5916 MB | 5918 MB | 12.8 s | - | - | - | Matches the README's 5.92 GB. Time was 9.25 s on a quiet machine. |
| 0a | - | - | Experiment: `MallocLargeCache=0` (1 run) | 4919 MB | - | 13.7 s | yes | - | no | Explains the missing gigabyte: allocator cache, not live data. An environment variable is not a fix for a phone. |
| 0b | - | - | Experiment: `MallocAggressiveMadvise=1` (1 run) | 5907 MB | - | 13.7 s | yes | - | no | No effect. |
| 0c | - | - | Experiment: 6 threads / 1 thread (1 run each) | 5527 / 5324 MB | - | 17.9 / 70.4 s | yes | - | no | Threads are not the cause: 0.6 GB from 18 threads to 1. |
| 1 | (this commit) | 0 | Sumcheck that owns its tables and folds them one at a time, instead of hyperplonk's three copies per round. Product check and batch opening ported onto it; the merged polynomials move into the sumcheck and g' is rebuilt from the committed polynomials. Evaluations for the openings read the tables in place. | 3101 MB | 3102 MB | 11.4 s | yes | G2 | yes | The copies were 2.8 GB of the 5.9. Faster too: less to copy. |
