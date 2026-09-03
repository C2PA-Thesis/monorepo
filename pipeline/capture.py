#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import secrets
import subprocess
from pathlib import Path

from lib.contracts import load_json, validate, write_json
from lib.images import canonicalize_capture
from lib.receipt import create_receipt, load_private_key, normalize_captured_at, utc_now


ROOT = Path(__file__).resolve().parents[1]


def hv_bin() -> str:
    return os.environ.get(
        "HV_BIN",
        str(ROOT / "editproof" / "hyperveritas_impl" / "target" / "release" / "hv"),
    )


def zkloc_bin() -> str:
    return os.environ.get(
        "ZKLOC_BIN", str(ROOT / "pipeline" / "generated" / "bin" / "zkloc")
    )


def fresh_salt() -> str:
    return "0x00" + secrets.token_hex(31)


def capture(
    photo: Path,
    latitude: float,
    longitude: float,
    device_key_path: Path,
    captured_at: str,
    out_dir: Path,
) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    original_png = out_dir / "original.png"
    original_json = out_dir / "original.json"
    fingerprint_path = out_dir / "fingerprint.json"
    envelope_path = out_dir / "envelope.json"
    secrets_path = out_dir / "secrets.json"

    canonicalize_capture(photo, original_png, original_json)
    subprocess.run(
        [hv_bin(), "hash", str(original_json), "--out", str(fingerprint_path)],
        check=True,
    )

    salt = fresh_salt()
    normalized_captured_at = normalize_captured_at(captured_at)
    private_inputs = {
        "schema_version": 1,
        "image_path": "original.json",
        "latitude_degrees": latitude,
        "longitude_degrees": longitude,
        "salt": salt,
        "captured_at": normalized_captured_at,
    }
    validate(private_inputs, "secrets")
    write_json(secrets_path, private_inputs)
    secrets_path.chmod(0o600)

    subprocess.run(
        [
            zkloc_bin(),
            "commit",
            "--lat",
            str(latitude),
            "--lon",
            str(longitude),
            "--salt",
            salt,
            "--out",
            str(envelope_path),
        ],
        check=True,
    )

    fingerprint = load_json(fingerprint_path)
    envelope = load_json(envelope_path)
    validate(fingerprint, "fingerprint")
    validate(envelope, "envelope")
    if fingerprint["original"] != {"rows": 1024, "cols": 512}:
        raise ValueError("HyperVerITAS fingerprint has the wrong image dimensions")

    receipt = create_receipt(
        fingerprint,
        envelope,
        normalized_captured_at,
        load_private_key(device_key_path),
    )
    write_json(out_dir / "receipt.json", receipt)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Canonicalize and bind one image capture to one private location"
    )
    parser.add_argument("photo", type=Path)
    parser.add_argument("--lat", type=float, required=True, dest="latitude")
    parser.add_argument("--lon", type=float, required=True, dest="longitude")
    parser.add_argument("--device-key", type=Path, required=True)
    parser.add_argument("--captured-at", default=None)
    parser.add_argument("--out", type=Path, required=True, dest="out_dir")
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    capture(
        args.photo,
        args.latitude,
        args.longitude,
        args.device_key,
        args.captured_at or utc_now(),
        args.out_dir,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
