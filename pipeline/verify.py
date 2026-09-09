#!/usr/bin/env python3
from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, Dict

from lib.c2pa import assertion_from_manifest, read_manifest
from lib.contracts import load_json, validate, write_json
from lib.images import CROP_SIZE, png_to_hv_json
from lib.presentation import announce
from lib.proofs import public_region, unpack_image_proof_bundle
from lib.receipt import load_public_key, verify_receipt


ROOT = Path(__file__).resolve().parents[1]


class CheckFailure(Exception):
    def __init__(self, check: int) -> None:
        self.check = check
        super().__init__("check {}".format(check))


def hv_bin() -> str:
    return os.environ.get(
        "HV_BIN",
        str(ROOT / "editproof" / "hyperveritas_impl" / "target" / "release" / "hv"),
    )


def zkloc_bin() -> str:
    return os.environ.get(
        "ZKLOC_BIN", str(ROOT / "pipeline" / "generated" / "bin" / "zkloc")
    )


def _run_check(command: list, check: int) -> None:
    try:
        subprocess.run(command, check=True, capture_output=True, text=True)
    except (OSError, subprocess.CalledProcessError) as error:
        raise CheckFailure(check) from error


def check_device(receipt: Dict[str, Any], public_key_path: Path) -> None:
    try:
        verify_receipt(receipt, load_public_key(public_key_path))
    except Exception as error:
        raise CheckFailure(1) from error


def check_image(
    signed_asset: Path,
    receipt: Dict[str, Any],
    image_proof: Dict[str, Any],
    hv_params: Path,
    temporary_root: Path,
) -> None:
    try:
        edited_json = temporary_root / "edited.json"
        fingerprint_path = temporary_root / "fingerprint.json"
        proof_dir = temporary_root / "pi_img"
        png_to_hv_json(signed_asset, edited_json, CROP_SIZE)
        write_json(fingerprint_path, receipt["fingerprint"])
        unpack_image_proof_bundle(image_proof, proof_dir)
        _run_check(
            [
                hv_bin(),
                "verify-crop",
                "--params",
                str(hv_params),
                "--edited",
                str(edited_json),
                "--fingerprint",
                str(fingerprint_path),
                "--proof",
                str(proof_dir),
            ],
            2,
        )
    except CheckFailure:
        raise
    except Exception as error:
        raise CheckFailure(2) from error


def check_location(
    receipt: Dict[str, Any],
    asserted_region: Dict[str, Any],
    requested_region: Dict[str, Any],
    location_proof: Dict[str, Any],
    zkloc_vk: Path,
    temporary_root: Path,
) -> None:
    try:
        validate(requested_region, "region")
        validate(location_proof, "pi_loc")
        if asserted_region != requested_region:
            raise ValueError("asserted region does not match requested region")
        if location_proof["public_inputs"]["envelope"] != receipt["envelope"]["value"]:
            raise ValueError("location proof envelope does not match receipt")
        if location_proof["public_inputs"]["region"] != public_region(requested_region):
            raise ValueError("location proof public region does not match requested region")
        proof_path = temporary_root / "pi_loc.json"
        region_path = temporary_root / "region.json"
        write_json(proof_path, location_proof)
        write_json(region_path, requested_region)
        _run_check(
            [
                zkloc_bin(),
                "verify",
                "--vk",
                str(zkloc_vk),
                "--proof",
                str(proof_path),
                "--envelope",
                receipt["envelope"]["value"],
                "--region",
                str(region_path),
            ],
            3,
        )
    except CheckFailure:
        raise
    except Exception as error:
        raise CheckFailure(3) from error


def verify_asset(
    signed_asset: Path,
    region_path: Path,
    device_public_key_path: Path,
    hv_params: Path,
    zkloc_vk: Path,
) -> None:
    announce("Read the C2PA manifest from the signed PNG and verify its signature and file integrity.")
    manifest = read_manifest(signed_asset)
    if manifest.get("validation_state") != "Valid":
        raise ValueError("C2PA validation_state is not Valid")
    assertion = assertion_from_manifest(manifest)
    validate(assertion, "assertion")
    requested_region = load_json(region_path)

    announce("C2PA is Valid. Check 1: verify the receipt signature with the trusted device public key.")
    check_device(assertion["receipt"], device_public_key_path)
    announce("Check 1 passed. Check 2: decode the published pixels and verify the HyperVerITAS crop proof.")
    with tempfile.TemporaryDirectory(prefix="zkloc-verify-") as temporary:
        temporary_root = Path(temporary)
        check_image(
            signed_asset,
            assertion["receipt"],
            assertion["image_proof"],
            hv_params,
            temporary_root,
        )
        announce("Check 2 passed. Check 3: verify the ZKLP proof against the receipt envelope and requested region.")
        check_location(
            assertion["receipt"],
            assertion["region"],
            requested_region,
            assertion["location_proof"],
            zkloc_vk,
            temporary_root,
        )
        announce("Check 3 passed. All reader checks accepted this signed PNG.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Verify C2PA validity, capture binding, image proof, and location proof"
    )
    parser.add_argument("signed", type=Path)
    parser.add_argument("--region", type=Path, required=True)
    parser.add_argument("--device-public-key", type=Path, required=True)
    parser.add_argument("--hv-params", type=Path, required=True)
    parser.add_argument("--zkloc-vk", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    try:
        verify_asset(
            args.signed,
            args.region,
            args.device_public_key,
            args.hv_params,
            args.zkloc_vk,
        )
    except CheckFailure as error:
        print("check {}".format(error.check), file=sys.stderr)
        return 1
    except Exception as error:
        print(str(error), file=sys.stderr)
        return 1
    print("check 1")
    print("check 2")
    print("check 3")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
