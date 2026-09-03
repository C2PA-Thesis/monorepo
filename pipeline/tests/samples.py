from __future__ import annotations

import base64
import copy
import hashlib
from pathlib import Path
from typing import Any, Dict

from cryptography.hazmat.primitives.asymmetric import ec

from lib.contracts import write_json
from lib.proofs import build_image_proof_bundle, public_region
from lib.receipt import create_receipt


ZERO = "0x" + "00" * 32
ONE = "0x" + "00" * 31 + "01"
HEX_DIGEST = "00" * 32


def fingerprint(value: str = ZERO) -> Dict[str, Any]:
    channel = [ZERO] * 128
    channel[0] = value
    return {
        "schema_version": 1,
        "algorithm": "hyperveritas-ajtai-chacha8-v1",
        "field": "bls12-381-scalar",
        "matrix": {
            "rows": 128,
            "row_seed": "ChaCha8Rng::seed_from_u64(row)",
        },
        "original": {"rows": 1024, "cols": 512},
        "channels": {"R": channel, "G": list(channel), "B": list(channel)},
    }


def envelope(value: str = ONE) -> Dict[str, Any]:
    return {
        "schema_version": 1,
        "algorithm": "mimc-bn254",
        "field": "bn254-scalar",
        "encoding": "f32-radians-ieee754-bits+salt-field",
        "value": value,
    }


def region(cell: str = "87c2e3020ffffff") -> Dict[str, Any]:
    return {
        "schema_version": 1,
        "scheme": "h3",
        "cell": cell,
        "resolution": 7,
        "face": 11,
        "i": 100,
        "j": 200,
        "k": 300,
    }


def location_proof(
    receipt_envelope: Dict[str, Any],
    proof_region: Dict[str, Any],
) -> Dict[str, Any]:
    return {
        "schema_version": 1,
        "proof_system": "groth16",
        "curve": "bn254",
        "proof": base64.b64encode(b"groth16-proof").decode("ascii"),
        "public_inputs": {
            "envelope": receipt_envelope["value"],
            "region": public_region(proof_region),
        },
        "parameters": {
            "id": HEX_DIGEST,
            "r1cs_sha256": HEX_DIGEST,
            "verifying_key_sha256": HEX_DIGEST,
        },
    }


def image_proof(proof_dir: Path, proof: bytes = b"pst-proof") -> Dict[str, Any]:
    proof_dir.mkdir(parents=True, exist_ok=True)
    digest = hashlib.sha256(proof).hexdigest()
    (proof_dir / "proof.bin").write_bytes(proof)
    write_json(
        proof_dir / "metadata.json",
        {
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
            "files": {"proof.bin": {"sha256": digest, "bytes": len(proof)}},
        },
    )
    return build_image_proof_bundle(proof_dir)


def signed_receipt(
    key: ec.EllipticCurvePrivateKey,
    fingerprint_value: str = ZERO,
    envelope_value: str = ONE,
) -> Dict[str, Any]:
    return create_receipt(
        fingerprint(fingerprint_value),
        envelope(envelope_value),
        "2026-09-02T12:00:00Z",
        key,
    )


def clone(value: Dict[str, Any]) -> Dict[str, Any]:
    return copy.deepcopy(value)
