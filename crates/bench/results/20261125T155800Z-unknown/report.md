# Benchmark run 20261125T155800Z-unknown (x86_64 m7i.2xlarge, PR 30 at b16ca0c)

- **Started:** 2026-11-25T15:58:00Z
- **Commit:** unknown on unknown
- **Uncommitted paths:** 0
- **Machine:** Intel(R) Xeon(R) Platinum 8488C, 8 logical cores (unknown performance), 33.08 GB
- **System:** Linux 7.0.0-1018-aws
- **Phone:** unknown
- **Power:** unknown, power mode unknown
- **Tools:** rustc 1.91.1 (ed61e7d7e 2025-11-07), go version go1.23.4 linux/amd64, node v22.11.0

## Health

725 samples kept, 0 failed, 3 of 61 scenarios skipped.

Skipped, phone-fingerprint-836x627-crop418x627-turn0-res7: set BENCH_IOS_TEAM to your Apple team id

Skipped, phone-envelope-836x627-crop418x627-turn0-res7: set BENCH_IOS_TEAM to your Apple team id

Skipped, phone-location-prove-836x627-crop418x627-turn0-res7: set BENCH_IOS_TEAM to your Apple team id

Median is the operation alone. Middle half is the 25th to 75th percentile. Process adds start-up and reading the inputs. CPU adds up all cores. Peak memory is the process's largest resident set.

## Pipeline at the reference case

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| capture to verified file | reference | 10 | 39.21 s | 39.06 s to 39.39 s | 39.23 s | 212.71 s | 4.67 GB | 207.1 KB |
| fingerprint | reference | 30 | 624.8 ms | 622.0 ms to 629.1 ms | 633.6 ms | 4.99 s | 129.7 MB |  |
| capture | reference | 30 | 632.5 ms | 630.8 ms to 635.1 ms | 641.7 ms | 4.99 s | 129.7 MB |  |
| location proof | reference | 30 | 864.8 ms | 863.1 ms to 869.2 ms | 873.4 ms | 5.83 s | 129.7 MB | 196 B |
| location verify | reference | 30 | 7.0 ms | 6.9 ms to 7.1 ms | 11.1 ms | 12.3 ms | 129.7 MB |  |
| crop proof | reference | 10 | 36.18 s | 36.14 s to 36.39 s | 36.22 s | 196.99 s | 4.70 GB | 36.7 KB |
| crop verify | reference | 10 | 1.44 s | 1.44 s to 1.45 s | 1.45 s | 4.88 s | 150.2 MB |  |
| C2PA sign | reference | 30 | 1.8 ms | 1.8 ms to 1.8 ms | 5.9 ms | 5.6 ms | 129.7 MB | 207.1 KB |
| reader verification | reference | 10 | 1.46 s | 1.45 s to 1.46 s | 1.46 s | 4.90 s | 150.2 MB | 207.1 KB |
| all attacks | reference | 10 | 11.65 s | 11.64 s to 11.69 s | 11.67 s | 61.53 s | 364.6 MB |  |

## Where the time goes

| Operation | Phase | Median | Share |
| --- | --- | ---: | ---: |
| capture to verified file | capture | 641.5 ms | 1.6% |
| capture to verified file | crop | 4.2 ms | 0.0% |
| capture to verified file | location proof | 866.7 ms | 2.2% |
| capture to verified file | crop proof | 36.26 s | 92.5% |
| capture to verified file | publish | 2.3 ms | 0.0% |
| capture to verified file | verify | 1.44 s | 3.7% |
| crop proof | load parameters | 570.9 ms | 1.6% |
| crop proof | prove | 35.61 s | 98.4% |
| crop verify | load parameters | 51 µs | 0.0% |
| crop verify | verify | 1.44 s | 100.0% |
| reader verification | load keys and parameters | 105 µs | 0.0% |
| reader verification | C2PA manifest | 1.8 ms | 0.1% |
| reader verification | device receipt | 292 µs | 0.0% |
| reader verification | crop proof | 1.45 s | 99.4% |
| reader verification | location proof | 7.2 ms | 0.5% |

## Image size sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fingerprint | 1024x512 (524288 pixels) | 30 | 624.8 ms | 622.0 ms to 629.1 ms | 633.6 ms | 4.99 s | 129.7 MB |  |
| crop proof | 1024x512 (524288 pixels) | 10 | 36.18 s | 36.14 s to 36.39 s | 36.22 s | 196.99 s | 4.70 GB | 36.7 KB |
| crop verify | 1024x512 (524288 pixels) | 10 | 1.44 s | 1.44 s to 1.45 s | 1.45 s | 4.88 s | 150.2 MB |  |
| fingerprint | 256x128 (32768 pixels) | 5 | 41.5 ms | 41.0 ms to 41.7 ms | 44.2 ms | 330.4 ms | 129.7 MB |  |
| crop proof | 256x128 (32768 pixels) | 5 | 18.24 s | 17.94 s to 18.29 s | 18.26 s | 78.51 s | 4.57 GB | 36.7 KB |
| crop verify | 256x128 (32768 pixels) | 5 | 297.8 ms | 297.2 ms to 301.8 ms | 302.5 ms | 568.9 ms | 129.7 MB |  |
| fingerprint | 256x256 (65536 pixels) | 5 | 81.8 ms | 80.5 ms to 81.9 ms | 85.1 ms | 639.9 ms | 129.7 MB |  |
| crop proof | 256x256 (65536 pixels) | 5 | 19.90 s | 19.87 s to 20.31 s | 19.93 s | 94.67 s | 4.55 GB | 36.7 KB |
| crop verify | 256x256 (65536 pixels) | 5 | 377.7 ms | 375.1 ms to 380.9 ms | 382.4 ms | 864.2 ms | 129.7 MB |  |
| fingerprint | 512x256 (131072 pixels) | 5 | 159.4 ms | 159.1 ms to 161.9 ms | 163.4 ms | 1.27 s | 129.7 MB |  |
| crop proof | 512x256 (131072 pixels) | 5 | 23.59 s | 23.24 s to 23.59 s | 23.61 s | 120.08 s | 4.58 GB | 36.7 KB |
| crop verify | 512x256 (131072 pixels) | 5 | 534.6 ms | 529.9 ms to 535.5 ms | 539.9 ms | 1.44 s | 129.7 MB |  |
| fingerprint | 512x512 (262144 pixels) | 5 | 318.8 ms | 315.0 ms to 320.2 ms | 324.4 ms | 2.55 s | 129.7 MB |  |
| crop proof | 512x512 (262144 pixels) | 5 | 28.84 s | 28.83 s to 29.11 s | 28.87 s | 153.96 s | 4.60 GB | 36.7 KB |
| crop verify | 512x512 (262144 pixels) | 5 | 836.9 ms | 835.6 ms to 846.9 ms | 842.7 ms | 2.59 s | 129.7 MB |  |
| fingerprint | 836x627 (524172 pixels) | 5 | 626.1 ms | 625.6 ms to 639.9 ms | 634.7 ms | 5.01 s | 129.7 MB |  |
| crop proof | 836x627 (524172 pixels) | 5 | 36.14 s | 36.09 s to 36.31 s | 36.18 s | 197.77 s | 4.72 GB | 36.7 KB |
| crop verify | 836x627 (524172 pixels) | 5 | 1.45 s | 1.45 s to 1.48 s | 1.46 s | 4.91 s | 150.1 MB |  |

## Crop size sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| crop proof | 512x512 (50.0% of the original) | 10 | 36.18 s | 36.14 s to 36.39 s | 36.22 s | 196.99 s | 4.70 GB | 36.7 KB |
| crop verify | 512x512 (50.0% of the original) | 10 | 1.44 s | 1.44 s to 1.45 s | 1.45 s | 4.88 s | 150.2 MB |  |
| crop proof | 64x64 (0.8% of the original) | 5 | 35.67 s | 35.65 s to 35.89 s | 35.71 s | 197.05 s | 4.65 GB | 36.7 KB |
| crop verify | 64x64 (0.8% of the original) | 5 | 764.9 ms | 764.5 ms to 774.8 ms | 769.4 ms | 4.23 s | 149.5 MB |  |
| crop proof | 256x256 (12.5% of the original) | 5 | 35.75 s | 35.72 s to 36.12 s | 35.79 s | 196.82 s | 4.67 GB | 36.7 KB |
| crop verify | 256x256 (12.5% of the original) | 5 | 929.9 ms | 924.9 ms to 936.9 ms | 934.9 ms | 4.39 s | 149.5 MB |  |
| crop proof | 1024x512 (100.0% of the original) | 5 | 36.90 s | 36.71 s to 37.16 s | 36.95 s | 197.78 s | 4.70 GB | 36.7 KB |
| crop verify | 1024x512 (100.0% of the original) | 5 | 2.15 s | 2.14 s to 2.15 s | 2.16 s | 5.61 s | 150.8 MB |  |

## Turn sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| crop proof | 0 degrees | 10 | 36.18 s | 36.14 s to 36.39 s | 36.22 s | 196.99 s | 4.70 GB | 36.7 KB |
| crop verify | 0 degrees | 10 | 1.44 s | 1.44 s to 1.45 s | 1.45 s | 4.88 s | 150.2 MB |  |
| crop proof | 90 degrees | 5 | 36.23 s | 36.16 s to 36.63 s | 36.27 s | 196.58 s | 4.68 GB | 36.7 KB |
| crop verify | 90 degrees | 5 | 1.44 s | 1.44 s to 1.45 s | 1.45 s | 4.90 s | 150.1 MB |  |
| crop proof | 180 degrees | 5 | 36.12 s | 36.09 s to 36.28 s | 36.16 s | 196.70 s | 4.69 GB | 36.7 KB |
| crop verify | 180 degrees | 5 | 1.45 s | 1.44 s to 1.46 s | 1.45 s | 4.89 s | 150.0 MB |  |
| crop proof | 270 degrees | 5 | 36.28 s | 36.10 s to 36.32 s | 36.32 s | 197.30 s | 4.69 GB | 36.7 KB |
| crop verify | 270 degrees | 5 | 1.44 s | 1.44 s to 1.45 s | 1.45 s | 4.90 s | 150.2 MB |  |

## H3 resolution sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| location proof | resolution 7 | 30 | 864.8 ms | 863.1 ms to 869.2 ms | 873.4 ms | 5.83 s | 129.7 MB | 196 B |
| location verify | resolution 7 | 30 | 7.0 ms | 6.9 ms to 7.1 ms | 11.1 ms | 12.3 ms | 129.7 MB |  |
| location proof | resolution 4 | 30 | 867.8 ms | 863.0 ms to 871.2 ms | 876.4 ms | 5.84 s | 129.7 MB | 196 B |
| location verify | resolution 4 | 30 | 7.1 ms | 6.9 ms to 7.2 ms | 11.2 ms | 12.4 ms | 129.7 MB |  |
| location proof | resolution 5 | 30 | 868.0 ms | 862.7 ms to 872.5 ms | 876.7 ms | 5.84 s | 129.7 MB | 196 B |
| location verify | resolution 5 | 30 | 7.0 ms | 6.9 ms to 7.1 ms | 11.1 ms | 12.5 ms | 129.7 MB |  |
| location proof | resolution 6 | 30 | 868.7 ms | 864.0 ms to 871.0 ms | 877.3 ms | 5.83 s | 129.7 MB | 196 B |
| location verify | resolution 6 | 30 | 7.0 ms | 6.9 ms to 7.1 ms | 11.1 ms | 12.4 ms | 129.7 MB |  |
| location proof | resolution 8 | 30 | 866.1 ms | 862.3 ms to 868.9 ms | 874.7 ms | 5.83 s | 129.7 MB | 196 B |
| location verify | resolution 8 | 30 | 7.0 ms | 6.8 ms to 7.1 ms | 11.1 ms | 12.5 ms | 129.7 MB |  |
| location proof | resolution 9 | 30 | 865.9 ms | 863.4 ms to 870.4 ms | 874.4 ms | 5.84 s | 129.7 MB | 196 B |
| location verify | resolution 9 | 30 | 7.1 ms | 7.0 ms to 7.2 ms | 11.1 ms | 12.4 ms | 129.7 MB |  |

## Thread sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| location proof | all cores | 30 | 864.8 ms | 863.1 ms to 869.2 ms | 873.4 ms | 5.83 s | 129.7 MB | 196 B |
| crop proof | all cores | 10 | 36.18 s | 36.14 s to 36.39 s | 36.22 s | 196.99 s | 4.70 GB | 36.7 KB |
| crop proof | 1 | 5 | 101.68 s | 101.09 s to 101.69 s | 101.72 s | 109.05 s | 4.29 GB | 36.7 KB |
| location proof | 1 | 5 | 3.40 s | 3.39 s to 3.40 s | 3.41 s | 3.42 s | 129.7 MB | 196 B |
| crop proof | 2 | 5 | 57.75 s | 57.70 s to 57.94 s | 57.78 s | 109.27 s | 4.35 GB | 36.7 KB |
| location proof | 2 | 5 | 1.79 s | 1.79 s to 1.79 s | 1.80 s | 3.45 s | 129.7 MB | 196 B |
| crop proof | 4 | 5 | 36.04 s | 35.82 s to 36.28 s | 36.08 s | 110.04 s | 4.44 GB | 36.7 KB |
| location proof | 4 | 5 | 999.9 ms | 996.6 ms to 1.01 s | 1.01 s | 3.45 s | 129.7 MB | 196 B |
| crop proof | 8 | 5 | 36.20 s | 36.12 s to 36.54 s | 36.24 s | 197.00 s | 4.66 GB | 36.7 KB |
| location proof | 8 | 5 | 868.8 ms | 868.1 ms to 871.7 ms | 877.4 ms | 5.86 s | 129.7 MB | 196 B |

## Baselines

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| upstream HyperVerITAS PST | reference | 10 | 41.40 s | 41.32 s to 41.45 s | 57.98 s | 264.58 s | 4.81 GB | 49.1 KB |
| upstream ZKLP Groth16 | reference | 10 | 131.3 ms | 129.2 ms to 133.8 ms | 5.05 s | 33.34 s | 140.9 MB |  |
| fingerprint, WebAssembly | reference | 30 | 9.99 s | 9.99 s to 10.01 s | 10.02 s | 10.03 s | 129.7 MB |  |

## On the iPhone

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fingerprint | 836x627 | 0 | skipped, see Health | | | | | |
| envelope | 836x627 | 0 | skipped, see Health | | | | | |
| location proof | 836x627 | 0 | skipped, see Health | | | | | |

## Against upstream and the papers

| Measure | This repository | Upstream, this machine | Paper |
| --- | ---: | ---: | ---: |
| Crop proof, prover | 35.61 s | 37.98 s | 16.10 s |
| Crop proof, verifier | 1.44 s | 3.41 s | 3.10 s |
| Crop proof, peak memory | 4.70 GB | 4.81 GB | 6.40 GB |
| Crop proof, size | 36.7 KB | 49.1 KB | 49.1 KB |
| Location proof, prover | 864.8 ms | 129.9 ms | 260.0 ms |
| Location proof, verifier | 7.0 ms | 1.2 ms | 1.5 ms |

The papers used other hardware: an Apple M3 with 36 GB for HyperVerITAS PST at 2^19 pixels, an m6i.xlarge with 4 vCPUs for ZKLP. This repository's location prover is a separate process that loads its keys each time, for proving and for verifying. Upstream's times the proof alone, in a process that already holds them. How much of the difference that accounts for is not measured.

## Files a prover or reader holds

| File | Size |
| --- | ---: |
| crop prover parameters | 201.3 MB |
| crop verifier parameters | 4144 B |
| location circuit | 3.5 MB |
| location proving key | 5.2 MB |
| location verifying key | 684 B |
