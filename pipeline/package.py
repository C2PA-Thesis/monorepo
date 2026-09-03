#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path
from typing import Any, Dict

from PIL import Image

from lib.c2pa import ASSERTION_LABEL, manifest_store_info, sign_asset
from lib.contracts import load_json, validate, write_json
from lib.images import CROP_SIZE
from lib.proofs import build_image_proof_bundle, public_region


def build_assertion(
    receipt: Dict[str, Any],
    region: Dict[str, Any],
    location_proof: Dict[str, Any],
    image_proof: Dict[str, Any],
) -> Dict[str, Any]:
    validate(receipt, "receipt")
    validate(region, "region")
    validate(location_proof, "pi_loc")
    validate(image_proof, "pi_img-bundle")
    if location_proof["public_inputs"]["envelope"] != receipt["envelope"]["value"]:
        raise ValueError("location proof envelope does not match the receipt")
    if location_proof["public_inputs"]["region"] != public_region(region):
        raise ValueError("location proof region does not match region.json")
    assertion = {
        "schema_version": 1,
        "receipt": receipt,
        "region": region,
        "location_proof": location_proof,
        "image_proof": image_proof,
        "binding": {
            "kind": "device-signed-public-input-pair",
            "device_asserted_time": True,
        },
    }
    validate(assertion, "assertion")
    return assertion


def build_manifest(assertion: Dict[str, Any]) -> Dict[str, Any]:
    return {
        "claim_version": 1,
        "claim_generator_info": [
            {"name": "td8-zkloc-pipeline", "version": "0.1.0"}
        ],
        "title": "Private location and image edit proof",
        "assertions": [
            {
                "label": "c2pa.actions",
                "data": {"actions": [{"action": "c2pa.cropped"}]},
            },
            {"label": ASSERTION_LABEL, "data": assertion},
        ],
    }


def package_asset(
    receipt_path: Path,
    region_path: Path,
    location_proof_path: Path,
    image_proof_dir: Path,
    edited_path: Path,
    output_path: Path,
) -> None:
    with Image.open(edited_path) as image:
        if image.format != "PNG" or image.size != CROP_SIZE:
            raise ValueError("edited asset must be a 512x512 PNG")

    assertion = build_assertion(
        load_json(receipt_path),
        load_json(region_path),
        load_json(location_proof_path),
        build_image_proof_bundle(image_proof_dir),
    )
    manifest = build_manifest(assertion)
    output_path.parent.mkdir(parents=True, exist_ok=True)
    manifest_path = output_path.parent / "manifest.json"
    report_path = output_path.parent / "c2patool-sign.txt"
    info_path = output_path.parent / "manifest-store.txt"
    write_json(manifest_path, manifest)
    sign_asset(edited_path, manifest_path, output_path, report_path)
    info_path.write_text(manifest_store_info(output_path), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Embed both proofs and the device receipt in a signed C2PA PNG"
    )
    parser.add_argument("--receipt", type=Path, required=True)
    parser.add_argument("--region", type=Path, required=True)
    parser.add_argument("--pi-loc", type=Path, required=True, dest="pi_loc")
    parser.add_argument("--pi-img", type=Path, required=True, dest="pi_img")
    parser.add_argument("--edited", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    package_asset(
        args.receipt,
        args.region,
        args.pi_loc,
        args.pi_img,
        args.edited,
        args.out,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
