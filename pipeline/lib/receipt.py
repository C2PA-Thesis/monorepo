from __future__ import annotations

import base64
import hashlib
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, Tuple

from cryptography.exceptions import InvalidSignature
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, utils

from .contracts import canonical_json, validate


def normalize_captured_at(value: str) -> str:
    candidate = value
    if candidate.endswith("Z"):
        candidate = candidate[:-1] + "+00:00"
    parsed = datetime.fromisoformat(candidate)
    if parsed.tzinfo is None:
        raise ValueError("captured_at must include an RFC3339 timezone")
    return (
        parsed.astimezone(timezone.utc)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z")
    )


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace(
        "+00:00", "Z"
    )


def key_id(public_key: ec.EllipticCurvePublicKey) -> str:
    der = public_key.public_bytes(
        encoding=serialization.Encoding.DER,
        format=serialization.PublicFormat.SubjectPublicKeyInfo,
    )
    return hashlib.sha256(der).hexdigest()


def signing_payload(receipt: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "schema_version": receipt["schema_version"],
        "fingerprint": receipt["fingerprint"],
        "envelope": receipt["envelope"],
        "rows": receipt["rows"],
        "cols": receipt["cols"],
        "captured_at": receipt["captured_at"],
        "device_key_id": receipt["device_key_id"],
    }


def _payload_digest(receipt: Dict[str, Any]) -> bytes:
    return hashlib.sha256(canonical_json(signing_payload(receipt))).digest()


def load_private_key(path: Path) -> ec.EllipticCurvePrivateKey:
    value = serialization.load_pem_private_key(path.read_bytes(), password=None)
    if not isinstance(value, ec.EllipticCurvePrivateKey) or not isinstance(
        value.curve, ec.SECP256R1
    ):
        raise ValueError("device key must be an ECDSA P-256 private key")
    return value


def load_public_key(path: Path) -> ec.EllipticCurvePublicKey:
    value = serialization.load_pem_public_key(path.read_bytes())
    if not isinstance(value, ec.EllipticCurvePublicKey) or not isinstance(
        value.curve, ec.SECP256R1
    ):
        raise ValueError("trusted device key must be an ECDSA P-256 public key")
    return value


def create_receipt(
    fingerprint: Dict[str, Any],
    envelope: Dict[str, Any],
    captured_at: str,
    private_key: ec.EllipticCurvePrivateKey,
) -> Dict[str, Any]:
    receipt = {
        "schema_version": 1,
        "fingerprint": fingerprint,
        "envelope": envelope,
        "rows": 1024,
        "cols": 512,
        "captured_at": normalize_captured_at(captured_at),
        "device_key_id": key_id(private_key.public_key()),
        "signature_algorithm": "ecdsa-p256-sha256",
    }
    signature = private_key.sign(
        _payload_digest(receipt),
        ec.ECDSA(utils.Prehashed(hashes.SHA256())),
    )
    receipt["signature"] = base64.b64encode(signature).decode("ascii")
    validate(receipt, "receipt")
    return receipt


def verify_receipt(
    receipt: Dict[str, Any],
    public_key: ec.EllipticCurvePublicKey,
) -> None:
    validate(receipt, "receipt")
    if receipt["device_key_id"] != key_id(public_key):
        raise InvalidSignature("untrusted device key id")
    try:
        signature = base64.b64decode(receipt["signature"], validate=True)
    except ValueError as error:
        raise InvalidSignature("invalid base64 signature") from error
    public_key.verify(
        signature,
        _payload_digest(receipt),
        ec.ECDSA(utils.Prehashed(hashes.SHA256())),
    )


def generate_key_pair(private_path: Path, public_path: Path) -> Tuple[str, str]:
    private_key = ec.generate_private_key(ec.SECP256R1())
    private_pem = private_key.private_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PrivateFormat.PKCS8,
        encryption_algorithm=serialization.NoEncryption(),
    )
    public_pem = private_key.public_key().public_bytes(
        encoding=serialization.Encoding.PEM,
        format=serialization.PublicFormat.SubjectPublicKeyInfo,
    )
    private_path.parent.mkdir(parents=True, exist_ok=True)
    private_path.write_bytes(private_pem)
    private_path.chmod(0o600)
    public_path.write_bytes(public_pem)
    return private_pem.decode("ascii"), public_pem.decode("ascii")
