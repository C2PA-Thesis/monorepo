#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import copy
import os
import subprocess
import sys
from pathlib import Path
from typing import Any, Dict, List

import h3
from PIL import Image

from capture import capture, zkloc_bin
from lib.c2pa import assertion_from_manifest, read_manifest, sign_asset
from lib.contracts import BLS12_381_SCALAR_MODULUS, load_json, validate, write_json
from lib.images import save_lossless_rgb
from lib.presentation import announce
from lib.receipt import create_receipt, key_id, load_private_key, utc_now
from package import build_manifest
from verify import verify_asset


PIPELINE_ROOT = Path(__file__).resolve().parent


def different_fingerprint(fingerprint: Dict[str, Any]) -> Dict[str, Any]:
    changed = copy.deepcopy(fingerprint)
    old_value = int(changed["channels"]["R"][0][2:], 16)
    new_value = (old_value + 1) % BLS12_381_SCALAR_MODULUS
    changed["channels"]["R"][0] = "0x{:064x}".format(new_value)
    validate(changed, "fingerprint")
    return changed


def package_mutation(
    case_dir: Path,
    assertion: Dict[str, Any],
    edited: Path,
) -> Path:
    validate(assertion, "assertion")
    case_dir.mkdir(parents=True, exist_ok=True)
    manifest_path = case_dir / "manifest.json"
    signed_path = case_dir / "signed.png"
    write_json(manifest_path, build_manifest(assertion))
    sign_asset(edited, manifest_path, signed_path, case_dir / "c2patool-sign.txt")
    manifest = read_manifest(signed_path)
    if manifest.get("validation_state") != "Valid":
        raise RuntimeError("mutated asset did not retain Valid C2PA state")
    return signed_path


def require_expected_check(
    signed_path: Path,
    expected_check: int,
    region_path: Path,
    device_public_key: Path,
    hv_params: Path,
    zkloc_params: Path,
) -> None:
    completed = subprocess.run(
        [
            sys.executable,
            str(PIPELINE_ROOT / "verify.py"),
            str(signed_path),
            "--region",
            str(region_path),
            "--device-public-key",
            str(device_public_key),
            "--hv-params",
            str(hv_params),
            "--zkloc-vk",
            str(zkloc_params),
        ],
        capture_output=True,
        text=True,
        env=dict(os.environ, PIPELINE_EXPLAIN="0"),
    )
    expected = "check {}".format(expected_check)
    if completed.returncode == 0 or completed.stdout or completed.stderr.strip() != expected:
        raise RuntimeError(
            "expected {}, got exit={} stdout={!r} stderr={!r}".format(
                expected,
                completed.returncode,
                completed.stdout,
                completed.stderr,
            )
        )


def record_packaged_case(
    results: List[Dict[str, Any]],
    name: str,
    assertion: Dict[str, Any],
    edited: Path,
    expected_check: int,
    out_dir: Path,
    region_path: Path,
    device_public_key: Path,
    hv_params: Path,
    zkloc_params: Path,
) -> None:
    announce("{}: package the changed evidence in a newly signed PNG. Expect check {} to reject it.".format(name, expected_check))
    signed_path = package_mutation(out_dir / name, assertion, edited)
    require_expected_check(
        signed_path,
        expected_check,
        region_path,
        device_public_key,
        hv_params,
        zkloc_params,
    )
    observed = "check {}".format(expected_check)
    announce("PASS: {}. C2PA remains Valid, and {} rejected the changed evidence.".format(name, observed))
    print("{}: {}".format(name, observed))
    results.append(
        {
            "name": name,
            "expected": observed,
            "observed": observed,
            "c2pa_validation_state": "Valid",
            "signed_asset": str(signed_path.relative_to(out_dir)),
        }
    )


def canonical_neighbor(region: Dict[str, Any], out_path: Path) -> Dict[str, Any]:
    neighbors = sorted(set(h3.grid_disk(region["cell"], 1)) - {region["cell"]})
    if not neighbors:
        raise RuntimeError("H3 did not return a neighboring cell")
    subprocess.run(
        [zkloc_bin(), "region", "--cell", neighbors[0], "--out", str(out_path)],
        check=True,
        capture_output=True,
        text=True,
    )
    neighbor = load_json(out_path)
    validate(neighbor, "region")
    return neighbor


def run(args: argparse.Namespace) -> Dict[str, Any]:
    args.out.mkdir(parents=True, exist_ok=True)
    announce("First, verify the unchanged signed PNG again as the baseline for these tests.")
    positive_manifest = read_manifest(args.valid / "signed.png")
    if positive_manifest.get("validation_state") != "Valid":
        raise RuntimeError("positive asset is not C2PA Valid")
    assertion = assertion_from_manifest(positive_manifest)
    validate(assertion, "assertion")
    verify_asset(
        args.valid / "signed.png",
        args.region,
        args.device_public_key,
        args.hv_params,
        args.zkloc_params,
    )

    private_key = load_private_key(args.device_private_key)
    if assertion["receipt"]["device_key_id"] != key_id(private_key.public_key()):
        raise RuntimeError("device private key does not match the valid receipt")

    results = []
    edited = args.valid / "edit" / "edited.png"

    announce("Test 1/6: change one byte of the device signature. The receipt check must reject it.")
    changed = copy.deepcopy(assertion)
    signature = bytearray(base64.b64decode(changed["receipt"]["signature"]))
    signature[-1] ^= 1
    changed["receipt"]["signature"] = base64.b64encode(signature).decode("ascii")
    record_packaged_case(
        results,
        "tampered-signature",
        changed,
        edited,
        1,
        args.out,
        args.region,
        args.device_public_key,
        args.hv_params,
        args.zkloc_params,
    )

    announce("Test 2/6: change one fingerprint value without a new device signature.")
    replacement_fingerprint = different_fingerprint(assertion["receipt"]["fingerprint"])
    changed = copy.deepcopy(assertion)
    changed["receipt"]["fingerprint"] = replacement_fingerprint
    record_packaged_case(
        results,
        "unsigned-fingerprint",
        changed,
        edited,
        1,
        args.out,
        args.region,
        args.device_public_key,
        args.hv_params,
        args.zkloc_params,
    )

    changed = copy.deepcopy(assertion)
    announce("Test 3/6: sign that changed fingerprint with the trusted demo key. The image proof must still reject it.")
    changed["receipt"] = create_receipt(
        replacement_fingerprint,
        assertion["receipt"]["envelope"],
        assertion["receipt"]["captured_at"],
        private_key,
    )
    record_packaged_case(
        results,
        "trusted-resigned-fingerprint",
        changed,
        edited,
        2,
        args.out,
        args.region,
        args.device_public_key,
        args.hv_params,
        args.zkloc_params,
    )

    announce("Test 4/6: request a neighboring H3 cell that does not contain the demo coordinate.")
    wrong_region_path = args.out / "wrong-neighbor-region.json"
    neighbor = canonical_neighbor(assertion["region"], wrong_region_path)
    wrong_proof_path = args.out / "wrong-neighbor-pi-loc.json"
    wrong_prove = subprocess.run(
        [
            zkloc_bin(),
            "prove",
            "--params",
            str(args.zkloc_params),
            "--secrets",
            str(args.valid / "capture" / "secrets.json"),
            "--region",
            str(wrong_region_path),
            "--out",
            str(wrong_proof_path),
        ],
        capture_output=True,
        text=True,
    )
    if wrong_prove.returncode != 0:
        refusal = (wrong_prove.stdout + wrong_prove.stderr).strip()
        if "does not map to declared region" not in refusal:
            raise RuntimeError("wrong-region prover failed for an unexpected reason: {}".format(refusal))
        print("wrong-neighbor-region: prove refused")
        announce("PASS: the location prover refused the wrong region before generating a proof.")
        results.append(
            {
                "name": "wrong-neighbor-region",
                "expected": "prove refused or check 3",
                "observed": "prove refused",
                "c2pa_validation_state": "not applicable",
                "cell": neighbor["cell"],
                "prover_error": refusal,
            }
        )
    else:
        changed = copy.deepcopy(assertion)
        changed["region"] = neighbor
        changed["location_proof"] = load_json(wrong_proof_path)
        record_packaged_case(
            results,
            "wrong-neighbor-region",
            changed,
            edited,
            3,
            args.out,
            wrong_region_path,
            args.device_public_key,
            args.hv_params,
            args.zkloc_params,
        )

    announce("Test 5/6: capture the same photo again with a fresh salt, then swap its receipt into the first bundle.")
    second_capture_dir = args.out / "second-capture"
    capture(
        args.photo,
        -34.5478,
        -58.4462,
        args.device_private_key,
        utc_now(),
        second_capture_dir,
    )
    second_receipt = load_json(second_capture_dir / "receipt.json")
    if second_receipt == assertion["receipt"]:
        raise RuntimeError("second capture receipt did not change")
    changed = copy.deepcopy(assertion)
    changed["receipt"] = second_receipt
    record_packaged_case(
        results,
        "swapped-second-capture-receipt",
        changed,
        edited,
        3,
        args.out,
        args.region,
        args.device_public_key,
        args.hv_params,
        args.zkloc_params,
    )

    announce("Test 6/6: change the red value of the top-left published pixel. Keep the original proof.")
    mutated_png = args.out / "unproven-pixel.png"
    with Image.open(edited) as image:
        rgb = image.convert("RGB")
        red, green, blue = rgb.getpixel((0, 0))
        rgb.putpixel((0, 0), ((red + 1) % 256, green, blue))
        save_lossless_rgb(rgb, mutated_png)
    record_packaged_case(
        results,
        "unproven-pixel",
        copy.deepcopy(assertion),
        mutated_png,
        2,
        args.out,
        args.region,
        args.device_public_key,
        args.hv_params,
        args.zkloc_params,
    )

    report = {
        "schema_version": 1,
        "positive_c2pa_validation_state": "Valid",
        "cases": results,
    }
    write_json(args.out / "results.json", report)
    announce("All six changes produced the expected rejection. Save negative/results.json.")
    return report


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run real packaged binding negatives")
    parser.add_argument("--valid", type=Path, required=True)
    parser.add_argument("--photo", type=Path, required=True)
    parser.add_argument("--region", type=Path, required=True)
    parser.add_argument("--device-private-key", type=Path, required=True)
    parser.add_argument("--device-public-key", type=Path, required=True)
    parser.add_argument("--hv-params", type=Path, required=True)
    parser.add_argument("--zkloc-params", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    run(parse_args())
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
