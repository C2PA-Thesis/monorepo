# Benchmark run 20261126T034428Z-07775e64 (lean crop prover, working tree of the memory hill-climb (uncommitted on 07775e6))

- **Started:** 2026-11-26T03:44:28Z
- **Commit:** 07775e6442ebc105d8fa94a5aded1a7e54f01946 on perf/crop-prover-memory
- **Uncommitted paths:** 51
- **Machine:** Apple M5 Max, 18 logical cores (6 performance), 137.44 GB
- **System:** macOS 26.6.2
- **Phone:** iPhone 13 mini, iOS 26.6.1
- **Power:** AC Power, power mode 0
- **Tools:** rustc 1.91.1 (ed61e7d7e 2025-11-07), go version go1.26.5 darwin/arm64, node v22.23.1

## Health

725 samples kept, 0 failed, 4 of 62 scenarios skipped.

Noisy, the middle half spans over 10% of the median: location-verify-1024x512-crop512x512-turn0-res7, location-verify-1024x512-crop512x512-turn0-res4, location-verify-1024x512-crop512x512-turn0-res8.

Skipped, phone-fingerprint-836x627-crop418x627-turn0-res7: the iPhone stayed locked

Skipped, phone-envelope-836x627-crop418x627-turn0-res7: the iPhone stayed locked

Skipped, phone-location-prove-836x627-crop418x627-turn0-res7: the iPhone stayed locked

Skipped, phone-crop-prove-1024x512-crop512x512-turn0-res7: the iPhone stayed locked

Median is the operation alone. Middle half is the 25th to 75th percentile. Process adds start-up and reading the inputs. CPU adds up all cores. Peak memory is the process's largest resident set; for the crop prover that counts its mapped key file, which its memory footprint does not. On the phone it is the footprint.

## Pipeline at the reference case

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| capture to verified file | reference | 10 | 5.58 s | 5.53 s to 5.59 s | 5.59 s | 62.41 s | 320.1 MB | 207.1 KB |
| fingerprint | reference | 30 | 371.3 ms | 368.3 ms to 373.8 ms | 384.2 ms | 6.06 s | 9.3 MB |  |
| capture | reference | 30 | 387.7 ms | 384.6 ms to 389.9 ms | 401.1 ms | 6.05 s | 14.8 MB |  |
| location proof | reference | 30 | 338.2 ms | 335.9 ms to 340.7 ms | 351.0 ms | 3.33 s | 93.6 MB | 196 B |
| location verify | reference | 30 | 16.5 ms | 15.0 ms to 16.8 ms | 24.1 ms | 23.6 ms | 16.2 MB |  |
| crop proof | reference | 10 | 4.16 s | 4.16 s to 4.16 s | 4.17 s | 49.99 s | 321.3 MB | 36.7 KB |
| crop verify | reference | 10 | 672.9 ms | 672.3 ms to 675.3 ms | 684.4 ms | 3.42 s | 108.6 MB |  |
| C2PA sign | reference | 30 | 2.3 ms | 2.2 ms to 2.4 ms | 8.7 ms | 7.6 ms | 11.2 MB | 207.1 KB |
| reader verification | reference | 10 | 683.9 ms | 681.6 ms to 684.8 ms | 688.1 ms | 3.40 s | 109.2 MB | 207.1 KB |
| all attacks | reference | 10 | 5.09 s | 5.05 s to 5.12 s | 5.10 s | 44.12 s | 113.7 MB |  |

## Where the time goes

| Operation | Phase | Median | Share |
| --- | --- | ---: | ---: |
| capture to verified file | capture | 391.4 ms | 7.0% |
| capture to verified file | crop | 2.5 ms | 0.0% |
| capture to verified file | location proof | 321.1 ms | 5.8% |
| capture to verified file | crop proof | 4.17 s | 74.9% |
| capture to verified file | publish | 1.7 ms | 0.0% |
| capture to verified file | verify | 678.1 ms | 12.2% |
| crop proof | load parameters | 355.2 ms | 8.5% |
| crop proof | prove | 3.80 s | 91.5% |
| crop verify | load parameters | 252 µs | 0.0% |
| crop verify | verify | 672.7 ms | 100.0% |
| reader verification | load keys and parameters | 319 µs | 0.0% |
| reader verification | C2PA manifest | 2.2 ms | 0.3% |
| reader verification | device receipt | 484 µs | 0.1% |
| reader verification | crop proof | 673.0 ms | 98.4% |
| reader verification | location proof | 8.0 ms | 1.2% |

## Image size sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fingerprint | 1024x512 (524288 pixels) | 30 | 371.3 ms | 368.3 ms to 373.8 ms | 384.2 ms | 6.06 s | 9.3 MB |  |
| crop proof | 1024x512 (524288 pixels) | 10 | 4.16 s | 4.16 s to 4.16 s | 4.17 s | 49.99 s | 321.3 MB | 36.7 KB |
| crop verify | 1024x512 (524288 pixels) | 10 | 672.9 ms | 672.3 ms to 675.3 ms | 684.4 ms | 3.42 s | 108.6 MB |  |
| fingerprint | 256x128 (32768 pixels) | 5 | 33.8 ms | 33.2 ms to 34.5 ms | 39.9 ms | 499.4 ms | 4.9 MB |  |
| crop proof | 256x128 (32768 pixels) | 5 | 1.67 s | 1.67 s to 1.68 s | 1.67 s | 17.52 s | 315.7 MB | 36.7 KB |
| crop verify | 256x128 (32768 pixels) | 5 | 163.3 ms | 162.0 ms to 163.4 ms | 171.0 ms | 458.4 ms | 99.0 MB |  |
| fingerprint | 256x256 (65536 pixels) | 5 | 51.0 ms | 50.5 ms to 51.4 ms | 57.4 ms | 835.6 ms | 5.2 MB |  |
| crop proof | 256x256 (65536 pixels) | 5 | 2.01 s | 1.98 s to 2.02 s | 2.02 s | 21.87 s | 317.5 MB | 36.7 KB |
| crop verify | 256x256 (65536 pixels) | 5 | 195.0 ms | 194.5 ms to 195.2 ms | 204.0 ms | 666.5 ms | 99.8 MB |  |
| fingerprint | 512x256 (131072 pixels) | 5 | 99.7 ms | 98.1 ms to 100.6 ms | 107.2 ms | 1.62 s | 5.8 MB |  |
| crop proof | 512x256 (131072 pixels) | 5 | 2.49 s | 2.48 s to 2.49 s | 2.49 s | 28.45 s | 317.7 MB | 36.7 KB |
| crop verify | 512x256 (131072 pixels) | 5 | 262.0 ms | 261.7 ms to 264.1 ms | 271.2 ms | 1.09 s | 100.8 MB |  |
| fingerprint | 512x512 (262144 pixels) | 5 | 189.6 ms | 188.2 ms to 197.7 ms | 199.1 ms | 3.13 s | 7.1 MB |  |
| crop proof | 512x512 (262144 pixels) | 5 | 3.27 s | 3.21 s to 3.28 s | 3.28 s | 38.19 s | 323.8 MB | 36.7 KB |
| crop verify | 512x512 (262144 pixels) | 5 | 397.7 ms | 397.1 ms to 403.9 ms | 407.7 ms | 1.86 s | 103.6 MB |  |
| fingerprint | 836x627 (524172 pixels) | 5 | 376.3 ms | 374.8 ms to 377.9 ms | 387.2 ms | 6.19 s | 9.3 MB |  |
| crop proof | 836x627 (524172 pixels) | 5 | 4.26 s | 4.19 s to 4.34 s | 4.28 s | 50.65 s | 319.6 MB | 36.7 KB |
| crop verify | 836x627 (524172 pixels) | 5 | 678.2 ms | 673.3 ms to 681.5 ms | 690.3 ms | 3.42 s | 108.9 MB |  |

## Crop size sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| crop proof | 512x512 (50.0% of the original) | 10 | 4.16 s | 4.16 s to 4.16 s | 4.17 s | 49.99 s | 321.3 MB | 36.7 KB |
| crop verify | 512x512 (50.0% of the original) | 10 | 672.9 ms | 672.3 ms to 675.3 ms | 684.4 ms | 3.42 s | 108.6 MB |  |
| crop proof | 64x64 (0.8% of the original) | 5 | 3.86 s | 3.84 s to 3.99 s | 3.88 s | 50.42 s | 319.9 MB | 36.7 KB |
| crop verify | 64x64 (0.8% of the original) | 5 | 317.5 ms | 314.5 ms to 319.4 ms | 325.7 ms | 3.13 s | 98.6 MB |  |
| crop proof | 256x256 (12.5% of the original) | 5 | 4.11 s | 3.96 s to 4.12 s | 4.13 s | 50.54 s | 319.3 MB | 36.7 KB |
| crop verify | 256x256 (12.5% of the original) | 5 | 401.8 ms | 399.7 ms to 402.4 ms | 410.8 ms | 3.19 s | 100.8 MB |  |
| crop proof | 1024x512 (100.0% of the original) | 5 | 4.65 s | 4.56 s to 4.81 s | 4.68 s | 51.19 s | 320.6 MB | 36.7 KB |
| crop verify | 1024x512 (100.0% of the original) | 5 | 1.05 s | 1.04 s to 1.05 s | 1.06 s | 3.79 s | 117.3 MB |  |

## Turn sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| crop proof | 0 degrees | 10 | 4.16 s | 4.16 s to 4.16 s | 4.17 s | 49.99 s | 321.3 MB | 36.7 KB |
| crop verify | 0 degrees | 10 | 672.9 ms | 672.3 ms to 675.3 ms | 684.4 ms | 3.42 s | 108.6 MB |  |
| crop proof | 90 degrees | 5 | 4.32 s | 4.23 s to 4.40 s | 4.33 s | 50.77 s | 321.8 MB | 36.7 KB |
| crop verify | 90 degrees | 5 | 678.9 ms | 676.1 ms to 683.7 ms | 689.6 ms | 3.44 s | 108.8 MB |  |
| crop proof | 180 degrees | 5 | 4.24 s | 4.21 s to 4.34 s | 4.25 s | 50.91 s | 319.3 MB | 36.7 KB |
| crop verify | 180 degrees | 5 | 675.1 ms | 673.1 ms to 676.5 ms | 687.8 ms | 3.45 s | 108.7 MB |  |
| crop proof | 270 degrees | 5 | 4.36 s | 4.21 s to 4.38 s | 4.37 s | 51.00 s | 320.2 MB | 36.7 KB |
| crop verify | 270 degrees | 5 | 677.3 ms | 676.1 ms to 680.0 ms | 687.8 ms | 3.45 s | 108.9 MB |  |

## H3 resolution sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| location proof | resolution 7 | 30 | 338.2 ms | 335.9 ms to 340.7 ms | 351.0 ms | 3.33 s | 93.6 MB | 196 B |
| location verify | resolution 7 | 30 | 16.5 ms | 15.0 ms to 16.8 ms | 24.1 ms | 23.6 ms | 16.2 MB |  |
| location proof | resolution 4 | 30 | 331.9 ms | 328.8 ms to 336.7 ms | 341.8 ms | 3.32 s | 92.8 MB | 196 B |
| location verify | resolution 4 | 30 | 16.5 ms | 14.5 ms to 16.7 ms | 23.8 ms | 23.3 ms | 16.2 MB |  |
| location proof | resolution 5 | 30 | 334.2 ms | 331.8 ms to 335.8 ms | 344.9 ms | 3.32 s | 93.4 MB | 196 B |
| location verify | resolution 5 | 30 | 16.4 ms | 15.9 ms to 16.8 ms | 24.2 ms | 23.7 ms | 16.2 MB |  |
| location proof | resolution 6 | 30 | 331.1 ms | 328.6 ms to 337.1 ms | 341.8 ms | 3.32 s | 92.5 MB | 196 B |
| location verify | resolution 6 | 30 | 16.6 ms | 15.9 ms to 16.9 ms | 24.4 ms | 23.7 ms | 16.2 MB |  |
| location proof | resolution 8 | 30 | 333.5 ms | 328.7 ms to 337.0 ms | 344.0 ms | 3.32 s | 92.8 MB | 196 B |
| location verify | resolution 8 | 30 | 16.4 ms | 14.6 ms to 16.6 ms | 23.9 ms | 23.5 ms | 16.2 MB |  |
| location proof | resolution 9 | 30 | 332.9 ms | 330.1 ms to 336.2 ms | 342.9 ms | 3.32 s | 93.9 MB | 196 B |
| location verify | resolution 9 | 30 | 16.3 ms | 15.2 ms to 16.7 ms | 23.5 ms | 23.0 ms | 16.2 MB |  |

## Thread sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| location proof | all cores | 30 | 338.2 ms | 335.9 ms to 340.7 ms | 351.0 ms | 3.33 s | 93.6 MB | 196 B |
| crop proof | all cores | 10 | 4.16 s | 4.16 s to 4.16 s | 4.17 s | 49.99 s | 321.3 MB | 36.7 KB |
| crop proof | 1 | 5 | 30.95 s | 30.91 s to 30.97 s | 30.97 s | 39.33 s | 312.2 MB | 36.7 KB |
| location proof | 1 | 5 | 2.82 s | 2.82 s to 2.82 s | 2.83 s | 2.83 s | 66.0 MB | 196 B |
| crop proof | 2 | 5 | 16.98 s | 16.96 s to 17.00 s | 17.00 s | 39.25 s | 312.1 MB | 36.7 KB |
| location proof | 2 | 5 | 1.50 s | 1.49 s to 1.50 s | 1.51 s | 2.90 s | 69.8 MB | 196 B |
| crop proof | 4 | 5 | 9.94 s | 9.92 s to 9.94 s | 9.96 s | 39.49 s | 316.1 MB | 36.7 KB |
| location proof | 4 | 5 | 821.2 ms | 818.1 ms to 821.4 ms | 834.3 ms | 2.92 s | 71.2 MB | 196 B |
| crop proof | 8 | 5 | 6.57 s | 6.56 s to 6.58 s | 6.59 s | 42.46 s | 313.5 MB | 36.7 KB |
| location proof | 8 | 5 | 519.9 ms | 519.2 ms to 523.4 ms | 532.1 ms | 3.08 s | 77.9 MB | 196 B |

## Baselines

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| upstream HyperVerITAS PST | reference | 10 | 12.89 s | 12.85 s to 12.91 s | 20.70 s | 95.97 s | 6.37 GB | 49.1 KB |
| upstream ZKLP Groth16 | reference | 10 | 41.4 ms | 40.4 ms to 42.3 ms | 1.62 s | 16.78 s | 218.8 MB |  |
| fingerprint, WebAssembly | reference | 30 | 5.76 s | 5.76 s to 5.77 s | 5.80 s | 5.80 s | 51.4 MB |  |

## On the iPhone

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fingerprint | 836x627 | 0 | skipped, see Health | | | | | |
| envelope | 836x627 | 0 | skipped, see Health | | | | | |
| location proof | 836x627 | 0 | skipped, see Health | | | | | |
| crop proof | 1024x512 | 0 | skipped, see Health | | | | | |

## Against upstream and the papers

| Measure | This repository | Upstream, this machine | Paper |
| --- | ---: | ---: | ---: |
| Crop proof, prover | 3.80 s | 10.37 s | 16.10 s |
| Crop proof, verifier | 672.7 ms | 2.53 s | 3.10 s |
| Crop proof, peak memory | 321.3 MB | 6.37 GB | 6.40 GB |
| Crop proof, size | 36.7 KB | 49.1 KB | 49.1 KB |
| Location proof, prover | 338.2 ms | 40.2 ms | 260.0 ms |
| Location proof, verifier | 16.5 ms | 1.2 ms | 1.5 ms |

The papers used other hardware: an Apple M3 with 36 GB for HyperVerITAS PST at 2^19 pixels, an m6i.xlarge with 4 vCPUs for ZKLP. This repository's location prover is a separate process that loads its keys each time, for proving and for verifying. Upstream's times the proof alone, in a process that already holds them. How much of the difference that accounts for is not measured.

## Files a prover or reader holds

| File | Size |
| --- | ---: |
| crop prover parameters | 201.3 MB |
| crop verifier parameters | 4144 B |
| location circuit | 3.5 MB |
| location proving key | 5.2 MB |
| location verifying key | 684 B |
