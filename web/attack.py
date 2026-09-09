from __future__ import annotations

import argparse
import base64
import copy
from pathlib import Path

from PIL import Image

from capture import capture
from integration_negatives import canonical_neighbor, package_mutation, require_expected_check
from lib.c2pa import assertion_from_manifest, read_manifest
from lib.contracts import load_json, write_json
from lib.images import save_lossless_rgb
from lib.presentation import announce
from lib.receipt import utc_now
from lib.workflow import ROOT, GENERATED


def run(directory, case):
    out = directory / "out"
    target = out / "attacks" / case
    target.mkdir(parents=True, exist_ok=True)
    assertion = copy.deepcopy(assertion_from_manifest(read_manifest(out / "signed.png")))
    edited = out / "edit/edited.png"
    region = directory / "region.json"
    if case == "signature":
        announce("Change one byte of the device signature, then sign a new C2PA PNG.")
        value = bytearray(base64.b64decode(assertion["receipt"]["signature"]))
        value[-1] ^= 1
        assertion["receipt"]["signature"] = base64.b64encode(value).decode()
        expected = 1
    elif case == "pixel":
        announce("Change the red value of one published pixel. Keep both original proofs.")
        with Image.open(edited) as image:
            image = image.convert("RGB")
            red, green, blue = image.getpixel((0, 0))
            image.putpixel((0, 0), ((red + 1) % 256, green, blue))
            edited = target / "changed.png"
            save_lossless_rgb(image, edited)
        expected = 2
    elif case == "region":
        announce("Ask the reader to verify the image against a neighboring region.")
        region = target / "requested-region.json"
        canonical_neighbor(assertion["region"], region)
        expected = 3
    else:
        announce("Capture this image again with a fresh salt. Swap in the new signed receipt.")
        secrets = load_json(out / "capture/secrets.json")
        capture(directory / "source.png", secrets["latitude_degrees"], secrets["longitude_degrees"],
                ROOT / "fixtures/generated/device-private.pem", utc_now(), target / "capture")
        assertion["receipt"] = load_json(target / "capture/receipt.json")
        expected = 3
    signed = package_mutation(target, assertion, edited)
    require_expected_check(signed, expected, region, ROOT / "fixtures/generated/device-public.pem",
                           GENERATED / "hv-pst", GENERATED / "zkloc-groth16")
    result = {"case": case, "expected": expected, "observed": expected,
              "c2pa": "Valid", "passed": True, "time": utc_now()}
    write_json(target / "result.json", result)
    announce("C2PA is Valid. Check {} rejected the change, as expected.".format(expected))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--run", type=Path, required=True)
    parser.add_argument("--case", choices=["pixel", "signature", "region", "receipt"], required=True)
    args = parser.parse_args()
    run(args.run, args.case)
