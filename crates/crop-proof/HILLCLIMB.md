# Memory hill-climb of the crop prover

Goal: run the crop prover on an iPhone 17 Pro. One row per attempt, kept or not.

- **Reference case**: 1024x512 original (2^19 pixels), 512x512 crop, no turn.
- **Metric**: peak memory footprint of the proving process, which is what
  `/usr/bin/time -l` prints as "peak memory footprint". Apple describes the
  footprint as "the dirty and compressed segments" (WWDC 2018 session 416,
  "iOS Memory Deep Dive"). This work takes it as the number an app's memory
  limit applies to; `os_proc_available_memory` returns "The number of bytes
  the app may allocate before it hits its memory limit" (Apple, os
  framework, Return Value), and the phone benchmark logs both. `bench probe`
  reads the footprint from the kernel right after the proof.
- **Tiers**: Tier 0 keeps the proof bytes. Tier 1 changes them without
  changing the protocol. Tier 2 changes the protocol. Every row is Tier 0.
- **Machine**: Apple M5 Max, 18 cores, 128 GB, macOS 26.6.2. All cores unless
  a row says otherwise. The machine was never idle (a VM and a system daemon
  held about one core), and other builds often ran at the same time. Compare
  times only where a row says the machine was quiet; footprints do not depend
  on the load.
- **Each number** is the median of 3 runs of `scripts/hillclimb-measure.sh`,
  which also verifies the proof it made. Experiments marked "1 run" ran once.
- **Proof hash**: the prover is deterministic, so for fixed parameters and
  inputs the proof bytes are fixed. `83ba522c65a2c842` is the first 16 hex
  digits of the SHA-256 of the reference proof under the parameters in this
  checkout's `.provenance`. A Tier 0 step must reproduce it.
- **Gates**: G1 `cargo test --workspace`; G2 clippy with attack hooks;
  G3 `cargo test --release -p crop-proof --features attack-hooks -- --ignored`
  (round trip, A1 to A3, and `same_proof`, which compares the proof with the
  reference prover's on three inputs, four since row 9); G4 `provenance setup && demo && attack`;
  G5 `forged_crops`.

## Result

Every step keeps the proof bytes (Tier 0 below), so the verifier's checks, the
transcript, the wire format and the soundness argument are the ones from
before. `tests/same_proof.rs` compares the proofs of the new prover with those
of the old one, which stays in the crate as `reference.rs` behind
`attack-hooks`. Two functions that the verifier shares with both provers were
rewritten, so that test cannot see a change in them; each has a test against
its old definition: `iop::range_tables`
(`range_tables_match_the_loop_they_replaced`) and `fingerprint::combine_rows`
(`combined_rows_match_the_definition`).

| Reference case, on the Mac | Before | After |
|---|---:|---:|
| Peak memory footprint | 5916 MB | 116 MB |
| Peak resident set | 5918 MB | 320 MB |
| Prove time, 18 cores | 12.8 s (9.25 s on a quiet machine) | 3.92 s |
| Proof | 36.7 KB | the same bytes |

The benchmark suite on a quiet machine (run `20261126T034428Z-07775e64`, 10
samples, each in its own process) gives 3.80 s to prove after 0.36 s to load
the key, against 8.74 s and 0.51 s for the prover before
(run `20261106T161115Z-01046cbe`). On one core the proof takes 30.95 s,
against 47.98 s.

The targets were 3.0 GB, 1.5 GB and 0.5 GB of footprint. The resident set is
above the footprint because the prover key (a 201 MB file) is a read-only
mapping: its pages are clean, and "Clean memory doesn't really count" toward
the footprint (Apple, WWDC 2018 session 416, "iOS Memory Deep Dive").

**Not measured on a phone.** The prover is built for iOS, linked into the app
and run by the phone benchmark (`crop-prove`), but only in the Simulator,
which is a Mac process. There the app's footprint peaked at 162 to 173 MB,
of which the app held 65 MB before the proof (row 9b). The iPhone 17 Pro was
not reachable while this was done. Until that run exists, "the crop proof
fits on the phone" is an expectation from the Mac's footprint, not a result.
One thing in particular is open: `DirectLarge` saves 77 MB in the Mac's
probe and nothing in the Simulator, so what it does on a phone is unknown.

### What worked

In the order of the ledger, with the footprint after each step:

1. **A sumcheck that owns its tables** (3101 MB). hyperplonk's prover copies
   every table in every round and keeps the caller's originals alive.
2. **Large allocations get their own mapping** (1756 MB). Apple's allocator
   keeps freed large blocks in a cache that counts toward the footprint; at
   that point the cache was 1.4 GB of the peak.
3. **Pixels stay bytes** (1757 MB at the peak, which was
   elsewhere, but the later steps build on it). A table of 2^20 field
   elements is 32 MiB; the same polynomial as bytes is 1 MiB, and sums over
   it bucket by byte value.
4. **A batch opening without tables** (977 MB). The eq polynomials are a
   running scalar times small outer-product tables, and since step 6 all
   quotients but the first are computed inside the multi-scalar
   multiplication that commits to them.
5. **A range check without tables** (725 MB), by the same means.
6. **An own multi-scalar multiplication** (484 MB), over chunks, with
   buckets in affine form. The proving time fell with it, from 8.8 s on a
   loaded machine to 3.5 s on a quiet one.
7. **A mapped prover key** (271 MB) instead of a deserialized one.
8. **Rebuilding the range tables instead of keeping them** (116 MB): each is
   a function of the bytes and two challenges, and cheap to build.

Three things made this safe to do quickly. The proof is deterministic, so
one SHA-256 says whether a change kept the transcript. Each rewritten part
has unit tests against hyperplonk at small sizes, transcript state included.
And the probe verifies the proof it measured, so a memory number cannot come
from a broken prover.

### What did not work

- Allocator settings from the environment (rows 0a, 0b): one explains the
  missing gigabyte, neither is a fix for an app.
- Fewer threads (0c): 0.6 GB of 5.9 GB.
- More lazy rounds in the zero check (8a): the live heap falls from 97 MB to
  83 MB and the footprint does not follow.
- A lower threshold for direct mappings (8b, 9a): 64 KiB is inside the
  noise; 8 KiB may gain about 13 MB, for one mapping per 8 KiB block.
- No change to the proof bytes or to the protocol (Tier 1 and Tier 2) was
  tried. None was needed for the targets.

### The next rung

- **Measure on the phone**, with the probe allocation test for the limit of
  the process, then let the app prove the crop and stop uploading the
  original.
- **The floor of this design** is the zero check of a range check: the
  fraction and the product, 32 MiB each, plus what the system allocator
  holds for blocks under 1 MiB (about 25 MiB at the end of a proof, row 8).
  Going under it means a sumcheck that streams its inputs. For one
  multilinear polynomial, Baweja et al. give "an algorithm that runs in time
  $O(kN)$ and uses space $O(N^{1/k})$" ("Time-Space Trade-Offs for Sumcheck",
  TCC 2025, abstract, [ePrint 2025/1473](https://eprint.iacr.org/2025/1473)).
  Scribe moves the state out of memory: "Instead of storing its (large)
  intermediate state in RAM, Scribe's prover instead stores it on disk"
  (Baweja et al., USENIX Security 2026, abstract,
  [ePrint 2024/1970](https://eprint.iacr.org/2024/1970)). Neither was tried
  here.
- **A larger original.** Memory no longer sets the 2^19-pixel bound; the
  parameters and the proving time do. Not tried.

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

Reading the code before the first measurement gave 4.4 to 4.9 GB of live
data in the range opening and did not explain the last gigabyte. The live heap peaks at 4.91 GB. The
other gigabyte is the system allocator's cache of freed large blocks: with
`MallocLargeCache=0` the footprint peak is 4919 MB, within 9 MB of the live
heap peak. After the proof the process still holds 2.2 GB of footprint for
0.58 GB of live heap. So the footprint follows the live heap plus up to a
gigabyte of recently freed large blocks, and cutting large allocate/free
cycles pays twice.

## Ledger

The Commit column is the position of the commit in the pull request, oldest
first, before the squash merge. Commit 3 only names types that the later
steps use.
A row that lists only G2 was measured between two full gate runs: all five
gates ran on the states of rows 1, 5, 7, 8, 9 and 9b.

| # | Commit | Tier | Idea | Footprint | RSS | Prove | Hash same | Gates | Kept | Lesson |
|---|---|---|---|---:|---:|---:|---|---|---|---|
| 0 | 1 | - | Baseline (`origin/main` plus the probe) | 5916 MB | 5918 MB | 12.8 s | - | - | - | Matches run `20261106T161115Z-01046cbe` of the benchmark (5.92 GB, and 9.25 s on a quiet machine). |
| 0a | - | - | Experiment: `MallocLargeCache=0` (1 run) | 4919 MB | - | 13.7 s | yes | - | no | Explains the missing gigabyte: allocator cache, not live data. An environment variable is not a fix for a phone. |
| 0b | - | - | Experiment: `MallocAggressiveMadvise=1` (1 run) | 5907 MB | - | 13.7 s | yes | - | no | No effect. |
| 0c | - | - | Experiment: 6 threads / 1 thread (1 run each) | 5527 / 5324 MB | - | 17.9 / 70.4 s | yes | - | no | Threads are not the cause: 0.6 GB from 18 threads to 1. |
| 1 | 2 | 0 | Sumcheck that owns its tables and folds them one at a time, instead of hyperplonk's three copies per round. Product check and batch opening ported onto it; the merged polynomials move into the sumcheck and g' is rebuilt from the committed polynomials. Evaluations for the openings read the tables in place. | 3101 MB | 3102 MB | 11.4 s | yes | G1 to G5 | yes | The copies were 2.8 GB of the 5.9. Faster too: less to copy. |
| 1a | - | - | Profile of step 1 (1 run) | 3098 MB | - | - | yes | - | - | Live heap peaks at 1731 MB, but the footprint climbs through the three range checks to 3000 MB while under 1 GB is live. The allocator cache is now 1.4 GB of the peak. |
| 2 | 4 | 0 | `crop_proof::alloc::DirectLarge` as the global allocator of `provenance` and `bench`: allocations of 1 MiB or more get their own mapping and are unmapped when freed. | 1756 MB | 1758 MB | 11.6 s (quiet), 13.9 to 18.9 s while other builds ran | yes | G2, G4, G5 | yes | The footprint now equals the live heap. No time cost: fresh mappings cost page faults, but so did the system allocator's large blocks. |
| 3 | 5 | 0 | Fingerprint and crop sumchecks read the pixels as bytes: the first rounds bucket the weights by pixel value, and only quarter-size folded tables are stored. `combine_rows` adds into the result block by block instead of one full vector per worker. | 1757 MB | 1759 MB | 11.6 s | yes | G2 | yes | The peak does not move: it is in the range opening. Measured in its own phases: the three sumchecks need 9 MiB instead of 40 MiB and run in 14 ms instead of 57 ms; `combine_rows` needs 16.6 MiB instead of 288 MiB. Kept because these phases become the peak once the openings and range checks shrink. |
| 4 | 6 | 0 | Batch opening without its tables (`lean/open.rs`). Before: one combined table and one eq table per distinct point (34 tables of 32 MiB in the range batch), a full g' and hyperplonk's copying KZG opening. Now: eq is a running scalar times two small tables; the combinations are never built because folding is linear, so only the 9 committed polynomials are folded, after 5 rounds computed from the originals; the quotients of the KZG opening are computed inside the MSM. Pixels are read as bytes. | 977 MB | 979 MB | (contended) | yes | G2 | yes | The module's own tables are about 10 MiB at 20 variables. What is left of this phase is the MSM's temporaries. The peak is now in the range checks. |
| 5 | 7 | 0 | Range check without its tables (`lean/range.rs`). Counts stay bytes; f and g are read from the bytes through 256-entry lookups; the fraction inverts g once per pair of bytes; p1 and p2 are read from the fraction and the product; eq is a scalar times two small tables; the first 3 rounds are computed from the originals and then 5 folded tables are kept at 1/8 size. The image is committed from its bytes. No field copy of the image exists any more. | 725 MB | 727 MB | 8.8 s (loaded machine) | yes | G1 to G5 | yes | Beyond the fraction and the product (64 MiB per channel, needed for the openings) the zero check holds 20 MiB. What is left is the commitment key in memory (218 MB), the six fractions and products (192 MiB) and the MSM's temporaries. |
| 6 | 8 | 0 | Own multi-scalar multiplication (`lean/msm.rs`). Byte scalars (pixels, counts): sum the basis points per byte value, one addition per non-zero byte, no allocation. Field scalars: Pippenger over chunks of 2^15 with digits of up to 15 bits stored as `i16`, buckets kept in affine form and filled with batched additions, scalars produced by a callback so quotients are never stored. arkworks made a bigint copy, 136 bytes of digits per scalar and 17 projective bucket arrays: about 250 MB per commit of 2^20. | 484 MB | 486 MB | 3.54 s (quiet machine) | yes | G2 | yes | 31 to 35 MB per MSM instead of 250 MB, and the MSM itself is 1.7 times faster; byte commitments take 23 ms instead of 326 ms. Against the baseline's 9.25 s on a quiet machine the prover is now 2.6 times faster. |
| 7 | 9 | 0 | The prover key is mapped read-only instead of deserialized (`Srs::open`). Its SHA-256 is checked on the mapped bytes, and each MSM decodes the points of the chunk it is working on. | 271 MB | 475 MB | 3.67 s | yes | G1 to G5 | yes | 218 MB of heap become clean file-backed pages, which the system can drop and reload and which do not count in the footprint (they still show in the RSS). Loading takes 0.35 s instead of 0.5 s. Decoding costs about 0.1 s over the whole proof. |
| 7a | - | - | Experiment: the C ABI check (`crates/crop-proof-ffi/c/check.sh`, two proofs and a verification in one process) with the system allocator and with `DirectLarge` (2 runs each, loaded machine) | 323 and 366 MB without; 278 MB with | 517 / 482 MB | - | same digest both ways | - | kept | The allocator cache still costs 45 to 90 MB at this size, and the figure varies from run to run without `DirectLarge`. The FFI library sets it as its global allocator; in the app that had no effect until row 9b. |
| 7b | 10 | - | `crop-proof-ffi`: C ABI, xcframework, Swift wrappers and a `crop-prove` op in the phone benchmark. Run in the iPhone 17 Pro Simulator only, which is a Mac process and not a measurement: proof digest equal to the Mac's, in-app peak 306 MB with the app's own 65 MB, native tests pass with both Rust libraries linked. That peak was taken with a library built from an earlier state of the prover; row 9b has the figure for the final one. | - | - | - | - | G2, FFI and bench tests | yes | The plumbing works. The phone's number is still missing: the iPhone was not reachable. |
| 8 | 11 | 0 | The fraction and the product of a range check are built again instead of kept (`lean/range.rs`). Both are functions of the pixels, the counts and two challenges, and `Lookup::fraction` and `Lookup::product` are the only code that builds them, so a table that is opened is the table that was committed to. The range check builds one table for each commitment and both for its zero check, and returns bytes only: its commitments, each paired with what an opening reads for it. The batch opening reads one polynomial at a time (`open::Source`) in each of its steps: the evaluations and the first rounds, the fold, the first quotient and the combination, which is now summed into a table source by source. Before, the tables of all three channels (192 MiB) lived until the range opening had read them. | 116 MB | 320 MB | 3.84 s (alternating with a build of step 7 in one session: 3.97 s against 3.78 s) | yes | G1 to G5 | yes | A table is cheap: 3 ms for a fraction and 6 ms for a product on 18 cores, 15 ms and 27 ms on one. Each is built 6 times per channel, about 0.17 s per proof on 18 cores. The live heap peaks at 97 MB whatever the number of threads, in the zero check of the last channel (both tables and 20 MiB of folded rows); the range opening needs 64 MB and the image opening 58 MB. The footprint is 14 to 25 MB above the live heap and moves between runs: 111 to 122 MB over 22 runs. It also depends mildly on the number of threads, which the live heap does not: 109 to 113 MB with 6 threads (7 runs) and 109 MB with 1 (4 runs). The likely cause is the system allocator, which serves every block under 1 MiB: at the end of the proof, with 9 MB live, `vmmap` shows 25 to 27 MiB dirty in its small zone with 18 threads, 9 to 14 MiB of it in regions with no live block, and 23 MiB with 1 thread; the stacks hold 1 MiB and 0.1 MiB (3 samples and 1). The first quotient of the final opening is computed from the polynomials, which reads each of them once more. That does not lower the peak of the proof, which is in the zero check. It keeps the range opening at 64 MB of heap instead of 80 MB and the image opening at 58 MB instead of 67 MB, for about 0.03 s (5 runs each, interleaved); more than one such quotient saves nothing. |
| 8a | - | 0 | Experiment: 5 lazy rounds in the zero check instead of 3 (`LAZY_ROUNDS` in `lean/range.rs`), 5 runs | 118 MB (105 to 119) | 322 MB | 4.30 s (loaded) | yes | - | no | The live heap peak falls from 97 MB to 83 MB (profile build, 2 runs; 6 rounds give 83 MB too, the two tables are the floor), but the footprint does not follow: its peak is no longer set by the live heap alone. Below about 120 MB the system allocator's share for blocks under 1 MiB decides, see row 8. |
| 8b | - | 0 | Experiment: `DirectLarge` threshold 64 KiB and 8 KiB instead of 1 MiB, 4 runs each | 111 MB (110 to 118) at 64 KiB; 103 MB (101 to 114) at 8 KiB | 315 / 307 MB | 4.1 to 5.6 s (machine loaded, not comparable) | yes | - | no | At 64 KiB the footprint is inside the run-to-run spread of step 8 (111 to 121 MB over 9 runs with 1 MiB). At 8 KiB it may gain about 13 MB, for one mapping per 8 KiB block. Not worth it at this level. |
| 9 | 12 | 0 | After three audits of the finished prover (equivalence with the reference prover, memory safety and robustness, code quality; no serious finding): `ProverParams::load` refuses a key over too few variables instead of panicking in `prove`; a point of the mapped key that stops decoding makes `prove` fail instead of returning a wrong proof; setup writes each parameter file to a new inode; `DirectLarge` maps only on Apple targets; the range tables have a test against the loop they replaced; `same_proof` has a fourth input with flat channels; one eq table type instead of two; `lean/dense.rs` removed; CI lints the `probe` and `mem-profile` features and runs the probe with `--verify`. | 116 MB (10 runs, 115 to 122) | 320 MB | 3.92 s | yes | G1 to G5 | yes | With 6 threads: 109 MB and 7.7 s; with 1 thread: 108 MB and 30.6 s (2 runs each). `DirectLarge` is worth 77 MB at this state (row 9a), so a binary that links the prover must install it to see these numbers. |
| 9a | - | 0 | Experiment: three builds of the final prover, 8 runs each, alternating, on a loaded machine: `DirectLarge` at 1 MiB (kept), at 64 KiB, and the system allocator | 113 MB at 1 MiB; 109 MB at 64 KiB; 190 MB with the system allocator | 317 / 313 / 394 MB | 7.9 to 20 s (loaded, not comparable) | yes | - | - | The threshold stays at 1 MiB: 64 KiB gains 3 MB, with ranges that overlap. Without `DirectLarge` the footprint is 77 MB higher. |
| 9b | 13 | 0 | The app runs the prover on `DirectLarge`. A review of the linked Simulator binary found that it did not: the app linked two Rust static libraries, both define the allocator symbols, and the linker kept those of the fingerprint library (the system allocator) without an error, because Xcode strips dead code. Now `crop-proof-ffi` carries `fingerprint-ffi`'s C functions and is the one Rust library of the app (`ProvenanceRust.xcframework`). A Swift test, the C check and a script that counts the archives defining `__rust_alloc` fail if this comes back. | - | - | - | yes | G1 to G5, app tests in the Simulator | yes | iPhone 17 Pro Simulator, `testMeasureACropProof`, peak footprint of the app up to the end of its first proof, 4 runs each: 162 to 173 MB with `DirectLarge`, 156 to 166 MB with the two-library link and the system allocator (three of those four at 156 MB). The app holds 65 MB before the proof. So the allocator gives no gain in the Simulator, unlike in the Mac's probe (row 9a); why was not investigated, and no device was measured. The old archives of row 7b, run again, gave 306 to 307 MB: that figure belonged to an earlier state of the prover, not to the allocator. |
