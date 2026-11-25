# bench

Measures the proof pipeline and writes a report. One command runs everything:

```bash
scripts/bench.sh
```

The command builds the code, runs `provenance setup`, and takes every sample.
The report is at `crates/bench/results/<run id>/report.md`. A full run takes
about 90 minutes.

| Command | What it does |
| --- | --- |
| `scripts/bench.sh` | The full suite |
| `scripts/bench.sh --quick` | Two samples of the reference operations, about two minutes. Use it as a check, not as a measurement |
| `scripts/bench.sh --group image-size` | One group. Repeat `--group` for several |
| `scripts/bench.sh --samples 3` | The same number of samples for every scenario |
| `scripts/bench.sh --label "before the change"` | Keeps a note with the run |
| `scripts/bench.sh --resume <run id>` | Continues a run that stopped, and retries what it skipped |
| `target/release/bench report <run dir>` | Writes the report again from the samples |
| `target/release/bench list` | Prints every scenario and its sample count |

For good numbers, connect the laptop to power and close other work. The run
waits up to five minutes for a quiet machine, and the report lists scenarios
that were noisy. To include the iPhone, connect it and unlock it. The run waits
two minutes for that, at the end. The script takes the Apple team id from
your development certificate. To use another team, set `BENCH_IOS_TEAM`.

## What it measures

The reference case is a 1024x512 original (2^19 pixels), a 512x512 crop, no
turn, and an H3 cell of resolution 7. It is the smallest case in the
HyperVerITAS paper, Table 3: "Full System Crop (50% of image)".

The paper figures in the report, as each paper states them:

| Figure | Source |
| --- | --- |
| HyperVerITAS PST at 2^19 pixels: prover 16.1 s, verifier 3.1 s, peak memory 6.4 GB, proof 49.1 KB | HyperVerITAS, Table 3, rows "HV[PST]", columns "TP (s)", "TV (s)", "MP (GB)" and "π (KB)" at 2^19. The paper's unit is "GB"; whether it means 10^9 or 2^30 bytes is not stated |
| HyperVerITAS hardware | §5.2: "laptop with 36GB RAM and an Apple M3 Chip" |
| ZKLP prover 0.26 s | ZKLP, Table 2, row "CZKLP FP32 Groth16", column "Prover Time" |
| ZKLP verifier 1.54 ms | ZKLP, §5: "Groth16 proof (≈ 1.54 ms)" |
| ZKLP hardware | ZKLP, §5: "an m6i.xlarge AWS instance with 4 vCPUs and 16 GB of RAM" |

| Group | Question it answers |
| --- | --- |
| `pipeline` | How long does each step take at the reference case? This group also measures capture to verified file, and all attacks |
| `image-size` | How does the cost change from 2^15 to 2^19 pixels, and at 836x627, the original of a 4:3 photo? |
| `crop-size` | How does the cost change from a 64x64 crop to the whole image? |
| `turn` | Does a quarter turn change the cost? |
| `resolution` | Does the H3 resolution change the cost of the location proof? |
| `threads` | What happens with 1, 2, 4 and 8 threads? |
| `baseline` | What do upstream HyperVerITAS and ZKLP cost on this machine? What does the fingerprint cost as WebAssembly? |
| `phone` | What do the fingerprint, the envelope and the location proof cost on a real iPhone? |

Each sweep changes one input. The other inputs stay at the reference case, so
a difference between two rows has one cause.

For each scenario the report gives:

| Column | Meaning |
| --- | --- |
| Median | The operation alone: the sum of its timed phases |
| Middle half | The 25th to the 75th percentile of that time |
| Process | The whole process, with start-up and reading the inputs |
| CPU | User and system time, added over all cores |
| Peak memory | The largest resident set of the process and its children. On the phone, the peak footprint of the test process so far |
| Output | The proof or file size |

The report also splits the main operations into phases. For example, it
separates loading the crop parameters from proving.

## How it measures

- **One process for each sample.** Peak memory comes from the operating system
  (`wait4`), and one sample cannot change the next.
- **Each operation checks its output.** A sample fails if the program fails or
  its output is wrong (a different envelope, a missing proof). The provers do
  not verify the proof they just made; the verify, reader and attack scenarios
  do.
- **Round 0 is a warmup.** It fills the file cache and is not in the statistics.
  So the numbers are for a warm cache.
- **The machine rests after each sample**, as long as the sample took, up to
  20 seconds. Without rests, heat slowed crop proofs from 10 s to 14 s in a run.
- **Rounds are interleaved.** Every scenario takes one sample, then every
  scenario takes the next. A slow change, such as heat, reaches all scenarios
  alike.
- **Real photo content.** Originals are the C2PA sample photo, resampled to each
  exact size.
- **Fixed inputs for the baselines.** HyperVerITAS gets the same pixels as this
  repository. Sources are pinned to one revision.
- **The phone pauses between rounds.** Each round is one capture, not a hot
  phone. Each sample records the thermal state.

## Results

Each run writes one directory in `crates/bench/results`:

| File | Contents |
| --- | --- |
| `report.md` | The tables, with machine, tools, commit and failures |
| `summary.csv` | One line for each scenario, for spreadsheets |
| `phases.csv` | One line for each phase of each scenario |
| `summary.json` | All statistics |
| `samples.jsonl` | Every raw sample, with its time and load average |
| `run.json` | The plan and the environment |

Fixtures and upstream builds go in `target/bench`.

## When something goes wrong

- A sample that fails, crashes or passes its timeout (15 minutes) is recorded,
  and the run continues. After three failures in a row a scenario stops.
- Each sample goes to disk when it ends. After a crash or a power loss,
  `--resume` continues and loses at most one sample.
- A missing nightly toolchain, Node, network or iPhone skips only the scenarios
  that need it. The report gives the reason.
- `bench run` exits with an error if a sample failed or a required scenario has
  no samples. CI runs it in this way.

## Limits

- Cold cache is not measured. Clearing it needs root.
- The thread limit applies to the proof libraries. The fingerprint inside the
  crop proof still uses all cores, so the 1-thread row is not a true single core.
- The upstream ZKLP times are in-process, with keys already loaded. This
  repository's location prover is a separate process that loads its keys.
- The WebAssembly row runs in Node on this laptop, on one thread. It is not
  Safari on a phone.
- The phone does not make crop proofs. No build of the crop proof for iOS exists.
- The phone signature is not measured, because it waits for Face ID.
- Two runs compare only on the same machine in the same state.
