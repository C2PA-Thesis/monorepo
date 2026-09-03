#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import re
from pathlib import Path

from lib.contracts import load_json


def format_bytes(size: int) -> str:
    return "{} bytes ({:.2f} KiB)".format(size, size / 1024)


def main() -> int:
    parser = argparse.ArgumentParser(description="Print PoC artifact sizes")
    parser.add_argument("--pi-img", type=Path, required=True)
    parser.add_argument("--pi-loc", type=Path, required=True)
    parser.add_argument("--manifest-info", type=Path, required=True)
    parser.add_argument("--signed", type=Path, required=True)
    args = parser.parse_args()

    pi_img_metadata = load_json(args.pi_img / "metadata.json")
    pst_size = sum(item["bytes"] for item in pi_img_metadata["files"].values())
    pi_loc = load_json(args.pi_loc)
    groth16_size = len(base64.b64decode(pi_loc["proof"], validate=True))
    info = args.manifest_info.read_text(encoding="utf-8")
    match = re.search(r"Manifest store size\s*=\s*(\d+)", info)

    print("artifact sizes")
    print("  HyperVerITAS PST proof: {}".format(format_bytes(pst_size)))
    print("  ZKLP Groth16 proof: {}".format(format_bytes(groth16_size)))
    if match:
        print("  C2PA manifest store: {}".format(format_bytes(int(match.group(1)))))
    else:
        print("  C2PA manifest store: not reported by c2patool")
    print("  final signed PNG: {}".format(format_bytes(args.signed.stat().st_size)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
