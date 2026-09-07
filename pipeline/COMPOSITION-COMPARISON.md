# Composition comparison for C2P-22

The first PoC chooses a device-signed pair of public inputs. It does not make
HyperVerITAS and ZKLP share one field commitment, and it does not recursively
combine their verifiers.

| Design | Binding mechanism | Field impact | Added verification cost | Component proof cost | Status |
| --- | --- | --- | --- | --- | --- |
| Signed public-input pair | P-256 signature over the HyperVerITAS fingerprint, ZKLP envelope, dimensions, and device-asserted time | None. HyperVerITAS stays on the BLS12-381 scalar field and ZKLP stays on BN254 | Signature only: 0.054 ms median. Complete receipt check: 5.427 ms median | PST: 153.010 s prover command, 3.807 s verifier, 36,692 B. Groth16: 0.539 s prover, 0.011 s median verifier, 196 B compressed proof | Wiring and honest-prover PoC measured. ZKLP malicious-prover soundness is blocked on unconstrained FP32 hint outputs |
| Same-commitment public-input join | Both proof statements expose one identical commitment | Requires a commitment definition and encoding that both fields can represent without ambiguous reduction | Unknown | Unknown, because neither current proof statement implements the shared commitment | Not implemented |
| Naive recursion | An outer proof verifies both inner proof verifiers | Requires in-circuit emulation across the BLS12-381 and BN254 pairing stacks | No full estimate claimed | At least the two inner proofs plus an outer proof | Rejected for this PoC |

The field reconciliation problem is structural. The PST image fingerprint is a
vector of BLS12-381 scalar elements. The location envelope is one BN254 scalar.
Equating them would require a new cross-field encoding or a new commitment in
one or both relations. A same-name JSON field would not make the cryptographic
statements share a commitment.

This comparison does not treat the current ZKLP result as a sound location
proof against a malicious prover. The FP32 circuit checks identities among
precomputed trigonometric hint outputs without constraining those outputs back
to the secret coordinates. The measured row establishes honest-prover behavior
and composition wiring only.

The recursion row is deliberately not assigned a fabricated total. The
[ZKLP paper, Appendix C(ii), page 19](https://arxiv.org/pdf/2404.14983v2)
states: "it requires 4·10⁶ constraints for in-circuit emulation over the circuit
unfriendly curve secp256k1 in gnark." This concerns authenticated GNSS signals.
It is a cited cost for emulated secp256k1 ECDSA, not this PoC's P-256 receipt
check or an estimate for recursively verifying PST and Groth16. The current
composition would need both pairing-based verifier stacks inside an outer relation.

The [HyperVerITAS paper, Section 5](https://crysp.petsymposium.org/popets/2026/popets-2026-0036.php)
reports: "our proof sizes for HyperVerITAS PST are between 49-55KB". That cited
paper result motivates PST instead of Brakedown for an inline assertion, but it
is not recorded as a measurement of this 2^19 PoC.

## Measurement boundary

`pipeline/run.sh` records component prover command wall times, the complete
reader verifier wall time, receipt microbenchmarks, and artifact sizes. The
individual proof-verifier timings in the table were collected separately;
the runner captures and discards those subprocess outputs.

The signed-pair row comes from the 2026-09-02 run on macOS 26.6.2 arm64 with
both fork binaries, real proof parameters, c2patool signing, and verification of
the published PNG. Signature medians use 100 in-process runs. The historical
Groth16 verifier median uses 30 subprocess runs, which the committed runner
does not repeat. Stubbed unit tests are not measurement evidence.
