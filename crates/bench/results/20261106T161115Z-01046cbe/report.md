# Benchmark run 20261106T161115Z-01046cbe (PR 21 stack head)

- **Started:** 2026-11-06T16:11:15Z
- **Commit:** 01046cbecd1d59d5af8a9c881852e54466a6e2a2 on patricioperaltarg/benchmark-harness
- **Uncommitted paths:** 9
- **Machine:** Apple M5 Max, 18 logical cores (6 performance), 137.44 GB
- **System:** macOS 26.6.2
- **Phone:** iPhone 17 Pro, iOS 27.0
- **Power:** AC Power, power mode 0
- **Tools:** rustc 1.91.1 (ed61e7d7e 2025-11-07), go version go1.26.5 darwin/arm64, node v22.23.1

## Health

755 samples kept, 0 failed, 0 of 61 scenarios skipped.

Noisy, the middle half spans over 10% of the median: location-verify-1024x512-crop512x512-turn0-res7, sign-1024x512-crop512x512-turn0-res7, location-verify-1024x512-crop512x512-turn0-res4, location-verify-1024x512-crop512x512-turn0-res6, location-verify-1024x512-crop512x512-turn0-res8, location-verify-1024x512-crop512x512-turn0-res9, phone-envelope-836x627-crop418x627-turn0-res7.

Median is the operation alone. Middle half is the 25th to 75th percentile. Process adds start-up and reading the inputs. CPU adds up all cores. Peak memory is the process's largest resident set.

## Pipeline at the reference case

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| capture to verified file | reference | 10 | 10.68 s | 10.56 s to 10.78 s | 10.71 s | 90.35 s | 5.92 GB | 207.1 KB |
| fingerprint | reference | 30 | 383.1 ms | 378.5 ms to 389.6 ms | 397.4 ms | 6.08 s | 10.1 MB |  |
| capture | reference | 30 | 394.5 ms | 390.1 ms to 400.0 ms | 408.5 ms | 6.08 s | 14.8 MB |  |
| location proof | reference | 30 | 335.1 ms | 330.5 ms to 337.6 ms | 349.4 ms | 3.37 s | 92.8 MB | 196 B |
| location verify | reference | 30 | 15.5 ms | 14.5 ms to 16.3 ms | 24.8 ms | 23.8 ms | 16.2 MB |  |
| crop proof | reference | 10 | 9.25 s | 9.23 s to 9.34 s | 9.29 s | 78.11 s | 5.92 GB | 36.7 KB |
| crop verify | reference | 10 | 711.1 ms | 704.4 ms to 718.6 ms | 727.1 ms | 3.43 s | 328.4 MB |  |
| C2PA sign | reference | 30 | 2.2 ms | 2.1 ms to 2.9 ms | 9.2 ms | 7.9 ms | 11.0 MB | 207.1 KB |
| reader verification | reference | 10 | 722.5 ms | 717.5 ms to 727.3 ms | 731.6 ms | 3.41 s | 329.2 MB | 207.1 KB |
| all attacks | reference | 10 | 5.44 s | 5.39 s to 5.52 s | 5.45 s | 43.93 s | 358.0 MB |  |

## Where the time goes

| Operation | Phase | Median | Share |
| --- | --- | ---: | ---: |
| capture to verified file | capture | 404.6 ms | 3.8% |
| capture to verified file | crop | 2.4 ms | 0.0% |
| capture to verified file | location proof | 317.6 ms | 3.0% |
| capture to verified file | crop proof | 9.22 s | 86.4% |
| capture to verified file | publish | 1.7 ms | 0.0% |
| capture to verified file | verify | 726.6 ms | 6.8% |
| crop proof | load parameters | 505.0 ms | 5.5% |
| crop proof | prove | 8.74 s | 94.5% |
| crop verify | load parameters | 173 µs | 0.0% |
| crop verify | verify | 710.9 ms | 100.0% |
| reader verification | load keys and parameters | 272 µs | 0.0% |
| reader verification | C2PA manifest | 2.0 ms | 0.3% |
| reader verification | device receipt | 398 µs | 0.1% |
| reader verification | crop proof | 710.8 ms | 98.4% |
| reader verification | location proof | 8.5 ms | 1.2% |

## Image size sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fingerprint | 1024x512 (524288 pixels) | 30 | 383.1 ms | 378.5 ms to 389.6 ms | 397.4 ms | 6.08 s | 10.1 MB |  |
| crop proof | 1024x512 (524288 pixels) | 10 | 9.25 s | 9.23 s to 9.34 s | 9.29 s | 78.11 s | 5.92 GB | 36.7 KB |
| crop verify | 1024x512 (524288 pixels) | 10 | 711.1 ms | 704.4 ms to 718.6 ms | 727.1 ms | 3.43 s | 328.4 MB |  |
| fingerprint | 256x128 (32768 pixels) | 5 | 32.2 ms | 30.9 ms to 33.0 ms | 41.0 ms | 491.9 ms | 4.9 MB |  |
| crop proof | 256x128 (32768 pixels) | 5 | 6.15 s | 6.14 s to 6.23 s | 6.17 s | 33.66 s | 5.90 GB | 36.7 KB |
| crop verify | 256x128 (32768 pixels) | 5 | 158.4 ms | 151.1 ms to 159.1 ms | 169.8 ms | 435.6 ms | 115.4 MB |  |
| fingerprint | 256x256 (65536 pixels) | 5 | 52.2 ms | 50.5 ms to 52.5 ms | 59.2 ms | 845.6 ms | 5.2 MB |  |
| crop proof | 256x256 (65536 pixels) | 5 | 6.49 s | 6.41 s to 6.50 s | 6.52 s | 39.22 s | 5.91 GB | 36.7 KB |
| crop verify | 256x256 (65536 pixels) | 5 | 193.5 ms | 185.5 ms to 193.9 ms | 204.6 ms | 651.5 ms | 136.0 MB |  |
| fingerprint | 512x256 (131072 pixels) | 5 | 99.6 ms | 95.1 ms to 102.3 ms | 108.9 ms | 1.56 s | 5.8 MB |  |
| crop proof | 512x256 (131072 pixels) | 5 | 6.98 s | 6.96 s to 7.03 s | 7.01 s | 47.41 s | 5.92 GB | 36.7 KB |
| crop verify | 512x256 (131072 pixels) | 5 | 264.8 ms | 262.8 ms to 265.9 ms | 280.2 ms | 1.05 s | 176.2 MB |  |
| fingerprint | 512x512 (262144 pixels) | 5 | 188.7 ms | 186.3 ms to 196.7 ms | 201.9 ms | 3.12 s | 7.4 MB |  |
| crop proof | 512x512 (262144 pixels) | 5 | 7.73 s | 7.71 s to 7.82 s | 7.76 s | 59.97 s | 5.92 GB | 36.7 KB |
| crop verify | 512x512 (262144 pixels) | 5 | 410.8 ms | 407.3 ms to 412.2 ms | 427.9 ms | 1.81 s | 255.5 MB |  |
| fingerprint | 836x627 (524172 pixels) | 5 | 372.8 ms | 372.7 ms to 378.1 ms | 386.7 ms | 6.13 s | 10.1 MB |  |
| crop proof | 836x627 (524172 pixels) | 5 | 9.14 s | 9.08 s to 9.74 s | 9.18 s | 77.41 s | 5.92 GB | 36.7 KB |
| crop verify | 836x627 (524172 pixels) | 5 | 707.5 ms | 706.0 ms to 708.6 ms | 725.1 ms | 3.41 s | 361.8 MB |  |

## Crop size sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| crop proof | 512x512 (50.0% of the original) | 10 | 9.25 s | 9.23 s to 9.34 s | 9.29 s | 78.11 s | 5.92 GB | 36.7 KB |
| crop verify | 512x512 (50.0% of the original) | 10 | 711.1 ms | 704.4 ms to 718.6 ms | 727.1 ms | 3.43 s | 328.4 MB |  |
| crop proof | 64x64 (0.8% of the original) | 5 | 8.75 s | 8.69 s to 9.01 s | 8.79 s | 76.79 s | 5.87 GB | 36.7 KB |
| crop verify | 64x64 (0.8% of the original) | 5 | 343.8 ms | 332.6 ms to 344.1 ms | 358.2 ms | 3.09 s | 315.9 MB |  |
| crop proof | 256x256 (12.5% of the original) | 5 | 8.80 s | 8.78 s to 9.20 s | 8.84 s | 76.88 s | 5.92 GB | 36.7 KB |
| crop verify | 256x256 (12.5% of the original) | 5 | 426.2 ms | 424.4 ms to 427.3 ms | 441.5 ms | 3.15 s | 318.7 MB |  |
| crop proof | 1024x512 (100.0% of the original) | 5 | 9.45 s | 9.40 s to 9.51 s | 9.49 s | 77.81 s | 5.92 GB | 36.7 KB |
| crop verify | 1024x512 (100.0% of the original) | 5 | 1.08 s | 1.08 s to 1.08 s | 1.10 s | 3.76 s | 339.8 MB |  |

## Turn sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| crop proof | 0 degrees | 10 | 9.25 s | 9.23 s to 9.34 s | 9.29 s | 78.11 s | 5.92 GB | 36.7 KB |
| crop verify | 0 degrees | 10 | 711.1 ms | 704.4 ms to 718.6 ms | 727.1 ms | 3.43 s | 328.4 MB |  |
| crop proof | 90 degrees | 5 | 9.04 s | 9.03 s to 9.17 s | 9.08 s | 77.14 s | 5.93 GB | 36.7 KB |
| crop verify | 90 degrees | 5 | 703.6 ms | 703.0 ms to 710.1 ms | 721.8 ms | 3.41 s | 328.9 MB |  |
| crop proof | 180 degrees | 5 | 9.02 s | 9.01 s to 9.06 s | 9.06 s | 76.85 s | 5.92 GB | 36.7 KB |
| crop verify | 180 degrees | 5 | 703.3 ms | 700.4 ms to 706.4 ms | 720.1 ms | 3.38 s | 329.0 MB |  |
| crop proof | 270 degrees | 5 | 9.01 s | 8.97 s to 9.06 s | 9.05 s | 77.02 s | 5.92 GB | 36.7 KB |
| crop verify | 270 degrees | 5 | 703.2 ms | 700.8 ms to 711.5 ms | 721.2 ms | 3.41 s | 328.0 MB |  |

## H3 resolution sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| location proof | resolution 7 | 30 | 335.1 ms | 330.5 ms to 337.6 ms | 349.4 ms | 3.37 s | 92.8 MB | 196 B |
| location verify | resolution 7 | 30 | 15.5 ms | 14.5 ms to 16.3 ms | 24.8 ms | 23.8 ms | 16.2 MB |  |
| location proof | resolution 4 | 30 | 329.7 ms | 324.3 ms to 337.1 ms | 342.5 ms | 3.36 s | 93.8 MB | 196 B |
| location verify | resolution 4 | 30 | 15.0 ms | 12.4 ms to 16.0 ms | 24.0 ms | 23.0 ms | 16.2 MB |  |
| location proof | resolution 5 | 30 | 332.5 ms | 326.9 ms to 340.1 ms | 342.8 ms | 3.37 s | 92.6 MB | 196 B |
| location verify | resolution 5 | 30 | 15.3 ms | 14.7 ms to 16.2 ms | 24.5 ms | 23.5 ms | 16.2 MB |  |
| location proof | resolution 6 | 30 | 330.5 ms | 326.5 ms to 334.5 ms | 341.7 ms | 3.36 s | 94.0 MB | 196 B |
| location verify | resolution 6 | 30 | 15.3 ms | 12.6 ms to 16.0 ms | 24.1 ms | 23.3 ms | 16.2 MB |  |
| location proof | resolution 8 | 30 | 331.5 ms | 326.6 ms to 337.3 ms | 342.6 ms | 3.36 s | 94.0 MB | 196 B |
| location verify | resolution 8 | 30 | 15.5 ms | 14.0 ms to 16.2 ms | 24.2 ms | 23.0 ms | 16.2 MB |  |
| location proof | resolution 9 | 30 | 330.0 ms | 325.6 ms to 336.8 ms | 341.2 ms | 3.37 s | 92.8 MB | 196 B |
| location verify | resolution 9 | 30 | 15.1 ms | 13.7 ms to 16.0 ms | 24.0 ms | 23.1 ms | 16.2 MB |  |

## Thread sweep

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| location proof | all cores | 30 | 335.1 ms | 330.5 ms to 337.6 ms | 349.4 ms | 3.37 s | 92.8 MB | 196 B |
| crop proof | all cores | 10 | 9.25 s | 9.23 s to 9.34 s | 9.29 s | 78.11 s | 5.92 GB | 36.7 KB |
| crop proof | 1 | 5 | 47.98 s | 47.93 s to 48.49 s | 48.02 s | 56.28 s | 5.33 GB | 36.7 KB |
| location proof | 1 | 5 | 2.89 s | 2.87 s to 2.89 s | 2.90 s | 2.89 s | 65.7 MB | 196 B |
| crop proof | 2 | 5 | 27.14 s | 27.09 s to 27.32 s | 27.18 s | 56.59 s | 5.37 GB | 36.7 KB |
| location proof | 2 | 5 | 1.51 s | 1.51 s to 1.51 s | 1.52 s | 2.92 s | 67.1 MB | 196 B |
| crop proof | 4 | 5 | 17.08 s | 16.88 s to 17.57 s | 17.13 s | 57.93 s | 5.45 GB | 36.7 KB |
| location proof | 4 | 5 | 831.2 ms | 822.1 ms to 844.9 ms | 843.5 ms | 2.93 s | 72.3 MB | 196 B |
| crop proof | 8 | 5 | 12.56 s | 12.49 s to 13.42 s | 12.60 s | 64.47 s | 5.61 GB | 36.7 KB |
| location proof | 8 | 5 | 523.5 ms | 523.0 ms to 532.3 ms | 538.3 ms | 3.16 s | 78.5 MB | 196 B |

## Baselines

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| upstream HyperVerITAS PST | reference | 10 | 13.49 s | 13.35 s to 13.75 s | 21.59 s | 99.22 s | 6.37 GB | 49.1 KB |
| upstream ZKLP Groth16 | reference | 10 | 43.5 ms | 42.8 ms to 44.0 ms | 1.68 s | 17.10 s | 206.1 MB |  |
| fingerprint, WebAssembly | reference | 30 | 5.89 s | 5.86 s to 5.96 s | 5.93 s | 5.92 s | 51.3 MB |  |

## On the iPhone

| Operation | Input | Samples | Median | Middle half | Process | CPU | Peak memory | Output |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| fingerprint | 836x627 | 10 | 1.85 s | 1.84 s to 1.86 s | 1.85 s | n/a | 89.2 MB |  |
| envelope | 836x627 | 10 | 299 µs | 263 µs to 304 µs | 299 µs | n/a | 89.2 MB |  |
| location proof | 836x627 | 10 | 1.33 s | 1.32 s to 1.34 s | 1.33 s | n/a | 89.2 MB | 196 B |

## Against upstream and the papers

| Measure | This repository | Upstream, this machine | Paper |
| --- | ---: | ---: | ---: |
| Crop proof, prover | 8.74 s | 10.92 s | 16.10 s |
| Crop proof, verifier | 710.9 ms | 2.59 s | 3.10 s |
| Crop proof, peak memory | 5.92 GB | 6.37 GB | 6.40 GB |
| Crop proof, size | 36.7 KB | 49.1 KB | 49.1 KB |
| Location proof, prover | 335.1 ms | 42.1 ms | 260.0 ms |
| Location proof, verifier | 15.5 ms | 1.3 ms | 1.5 ms |

The papers used other hardware: an Apple M3 with 36 GB for HyperVerITAS PST at 2^19 pixels, an m6i.xlarge with 4 vCPUs for ZKLP. This repository's location prover is a separate process that loads its keys each time. Upstream's times the proof alone, in a process that already holds them.

## Files a prover or reader holds

| File | Size |
| --- | ---: |
| crop prover parameters | 201.3 MB |
| crop verifier parameters | 4144 B |
| location circuit | 3.5 MB |
| location proving key | 5.2 MB |
| location verifying key | 684 B |
