#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from lib.images import crop_left_half


def crop(capture_dir: Path, out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    crop_left_half(
        capture_dir / "original.png",
        out_dir / "edited.png",
        out_dir / "edited.json",
    )


def main() -> int:
    parser = argparse.ArgumentParser(description="Apply the fixed 50 percent left crop")
    parser.add_argument("--capture", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True, dest="out_dir")
    args = parser.parse_args()
    crop(args.capture, args.out_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
