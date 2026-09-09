#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import hashlib
import statistics
import time
from pathlib import Path

from cryptography.hazmat.primitives import hashes
from cryptography.hazmat.primitives.asymmetric import ec, utils

from lib.contracts import canonical_json, load_json, write_json
from lib.presentation import announce
from lib.receipt import load_public_key, signing_payload, verify_receipt


def median_seconds(operation, iterations: int) -> float:
    samples = []
    for _ in range(iterations):
        started = time.perf_counter_ns()
        operation()
        samples.append((time.perf_counter_ns() - started) / 1_000_000_000)
    return statistics.median(samples)


def main() -> int:
    parser = argparse.ArgumentParser(description="Measure device receipt verification")
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--device-public-key", type=Path, required=True)
    parser.add_argument("--iterations", type=int, default=100)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    if args.iterations < 1:
        raise ValueError("iterations must be positive")

    receipt = load_json(args.receipt)
    public_key = load_public_key(args.device_public_key)
    signature = base64.b64decode(receipt["signature"], validate=True)
    digest = hashlib.sha256(canonical_json(signing_payload(receipt))).digest()

    def signature_only() -> None:
        public_key.verify(
            signature,
            digest,
            ec.ECDSA(utils.Prehashed(hashes.SHA256())),
        )

    def complete_receipt_check() -> None:
        verify_receipt(receipt, public_key)

    signature_only()
    complete_receipt_check()
    result = {
        "iterations": args.iterations,
        "signature_only_median_seconds": median_seconds(signature_only, args.iterations),
        "complete_receipt_check_median_seconds": median_seconds(
            complete_receipt_check, args.iterations
        ),
    }
    write_json(args.out, result)
    announce("Median signature check: {:.3f} ms. Median complete receipt check: {:.3f} ms.".format(
        result["signature_only_median_seconds"] * 1000,
        result["complete_receipt_check_median_seconds"] * 1000,
    ))
    print(
        "device signature median: {:.6f} seconds".format(
            result["signature_only_median_seconds"]
        )
    )
    print(
        "complete receipt check median: {:.6f} seconds".format(
            result["complete_receipt_check_median_seconds"]
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
