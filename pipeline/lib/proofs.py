from __future__ import annotations

import base64
import hashlib
from pathlib import Path
from typing import Any, Dict

from .contracts import load_json, validate, write_json


def sha256_hex(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def _safe_member_name(name: str) -> None:
    if not name or Path(name).name != name or name in {".", ".."}:
        raise ValueError("unsafe proof member name")


def build_image_proof_bundle(proof_dir: Path) -> Dict[str, Any]:
    metadata = load_json(proof_dir / "metadata.json")
    validate(metadata, "pi_img-metadata")
    members = {}
    for name, expected in metadata["files"].items():
        _safe_member_name(name)
        value = (proof_dir / name).read_bytes()
        digest = sha256_hex(value)
        if digest != expected["sha256"] or len(value) != expected["bytes"]:
            raise ValueError("image proof member checksum mismatch: {}".format(name))
        members[name] = {
            "encoding": "base64",
            "data": base64.b64encode(value).decode("ascii"),
            "sha256": digest,
            "bytes": len(value),
        }
    bundle = {"schema_version": 1, "metadata": metadata, "members": members}
    validate(bundle, "pi_img-bundle")
    return bundle


def unpack_image_proof_bundle(bundle: Dict[str, Any], out_dir: Path) -> None:
    validate(bundle, "pi_img-bundle")
    out_dir.mkdir(parents=True, exist_ok=True)
    metadata = bundle["metadata"]
    for name, expected in metadata["files"].items():
        _safe_member_name(name)
        member = bundle["members"].get(name)
        if member is None:
            raise ValueError("missing image proof member: {}".format(name))
        try:
            value = base64.b64decode(member["data"], validate=True)
        except ValueError as error:
            raise ValueError("invalid image proof base64: {}".format(name)) from error
        digest = sha256_hex(value)
        if (
            digest != member["sha256"]
            or digest != expected["sha256"]
            or len(value) != member["bytes"]
            or len(value) != expected["bytes"]
        ):
            raise ValueError("image proof member checksum mismatch: {}".format(name))
        (out_dir / name).write_bytes(value)
    write_json(out_dir / "metadata.json", metadata)


def public_region(region: Dict[str, Any]) -> Dict[str, int]:
    return {
        "resolution": region["resolution"],
        "face": region["face"],
        "i": region["i"],
        "j": region["j"],
        "k": region["k"],
    }
