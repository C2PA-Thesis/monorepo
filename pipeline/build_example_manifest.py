#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import hashlib
from pathlib import Path

from lib.contracts import validate, write_json
from package import build_manifest


ZERO = "0x" + "00" * 32
ZERO_DIGEST = "00" * 32


def main() -> int:
    parser = argparse.ArgumentParser(description="Build the tracked assertion shape example")
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    channel = [ZERO] * 128
    fingerprint = {
        "schema_version": 1,
        "algorithm": "hyperveritas-ajtai-chacha8-v1",
        "field": "bls12-381-scalar",
        "matrix": {
            "rows": 128,
            "row_seed": "ChaCha8Rng::seed_from_u64(row)",
        },
        "original": {"rows": 1024, "cols": 512},
        "channels": {"R": channel, "G": channel, "B": channel},
    }
    envelope = {
        "schema_version": 1,
        "algorithm": "mimc-bn254",
        "field": "bn254-scalar",
        "encoding": "f32-radians-ieee754-bits+salt-field",
        "value": ZERO,
    }
    region = {
        "schema_version": 1,
        "scheme": "h3",
        "cell": "87c2e3020ffffff",
        "resolution": 7,
        "face": 8,
        "i": 1816,
        "j": 0,
        "k": 736,
    }
    member = b"\x00"
    member_digest = hashlib.sha256(member).hexdigest()
    assertion = {
        "schema_version": 1,
        "receipt": {
            "schema_version": 1,
            "fingerprint": fingerprint,
            "envelope": envelope,
            "rows": 1024,
            "cols": 512,
            "captured_at": "2026-09-02T12:00:00Z",
            "device_key_id": ZERO_DIGEST,
            "signature_algorithm": "ecdsa-p256-sha256",
            "signature": "AA==",
        },
        "region": region,
        "location_proof": {
            "schema_version": 1,
            "proof_system": "groth16",
            "curve": "bn254",
            "proof": "AA==",
            "public_inputs": {
                "envelope": ZERO,
                "region": {
                    key: region[key] for key in ("resolution", "face", "i", "j", "k")
                },
            },
            "parameters": {
                "id": ZERO_DIGEST,
                "r1cs_sha256": ZERO_DIGEST,
                "verifying_key_sha256": ZERO_DIGEST,
            },
        },
        "image_proof": {
            "schema_version": 1,
            "metadata": {
                "schema_version": 1,
                "protocol": "hyperveritas-pst-crop-v1",
                "size": 19,
                "original": {"rows": 1024, "cols": 512},
                "crop": {
                    "start_row": 0,
                    "start_col": 0,
                    "end_row": 512,
                    "end_col": 512,
                },
                "files": {
                    "proof.bin": {"sha256": member_digest, "bytes": len(member)}
                },
            },
            "members": {
                "proof.bin": {
                    "encoding": "base64",
                    "data": base64.b64encode(member).decode("ascii"),
                    "sha256": member_digest,
                    "bytes": len(member),
                }
            },
        },
        "binding": {
            "kind": "device-signed-public-input-pair",
            "device_asserted_time": True,
        },
    }
    validate(assertion, "assertion")
    write_json(args.out, build_manifest(assertion))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
