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
| 1 | 07775e6 | 0 | Sumcheck that owns its tables and folds them one at a time, instead of hyperplonk's three copies per round. Product check and batch opening ported onto it; the merged polynomials move into the sumcheck and g' is rebuilt from the committed polynomials. Evaluations for the openings read the tables in place. | 3101 MB | 3102 MB | 11.4 s | yes | G2 | yes | The copies were 2.8 GB of the 5.9. Faster too: less to copy. |
| 1a | - | - | Profile of step 1 (1 run) | 3098 MB | - | - | yes | - | - | Live heap peaks at 1731 MB, but the footprint climbs through the three range checks to 3000 MB while under 1 GB is live. The allocator cache is now 1.4 GB of the peak. |
| 2 | (pending) | 0 | `crop_proof::alloc::DirectLarge` as the global allocator of `provenance` and `bench`: allocations of 1 MiB or more get their own mapping and are unmapped when freed. | 1756 MB | 1758 MB | 11.6 s (quiet), 13.9 to 18.9 s while other builds ran | yes | G2 | yes | The footprint now equals the live heap. No time cost: fresh mappings cost page faults, but so did the system allocator's large blocks. |
| 3 | (pending) | 0 | Fingerprint and crop sumchecks read the pixels as bytes: the first rounds bucket the weights by pixel value, and only quarter-size folded tables are stored. `combine_rows` adds into the result block by block instead of one full vector per worker. | 1757 MB | 1759 MB | 11.6 s | yes | G2 | yes | The peak does not move: it is in the range opening. Measured in its own phases: the three sumchecks need 9 MiB instead of 40 MiB and run in 14 ms instead of 57 ms; `combine_rows` needs 16.6 MiB instead of 288 MiB. Kept because these phases become the peak once the openings and range checks shrink. |
| 4 | (pending) | 0 | Batch opening without its tables (`lean/open.rs`). Before: one combined table and one eq table per distinct point (34 tables of 32 MiB in the range batch), a full g' and hyperplonk's copying KZG opening. Now: eq is a running scalar times two small tables; the combinations are never built because folding is linear, so only the 9 committed polynomials are folded, after 5 rounds computed from the originals; the quotients of the KZG opening are computed inside the MSM. Pixels are read as bytes. | 977 MB | 979 MB | (contended) | yes | G2 | yes | The module's own tables are about 10 MiB at 20 variables. What is left of this phase is the MSM's temporaries. The peak is now in the range checks. |
| 5 | (pending) | 0 | Range check without its tables (`lean/range.rs`). Counts stay bytes; f and g are read from the bytes through 256-entry lookups; the fraction inverts g once per pair of bytes; p1 and p2 are read from the fraction and the product; eq is a scalar times two small tables; the first 3 rounds are computed from the originals and then 5 folded tables are kept at 1/8 size. The image is committed from its bytes. No field copy of the image exists any more. | 725 MB | 727 MB | 8.8 s (loaded machine) | yes | G2 | yes | Beyond the fraction and the product (64 MiB per channel, needed for the openings) the zero check holds 20 MiB. What is left is the commitment key in memory (218 MB), the six fractions and products (192 MiB) and the MSM's temporaries. |
