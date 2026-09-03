#!/usr/bin/env python3
from __future__ import annotations

import argparse
from pathlib import Path

from lib.receipt import generate_key_pair


def main() -> int:
    parser = argparse.ArgumentParser(description="Generate a demo P-256 device key pair")
    parser.add_argument("--private", type=Path, required=True)
    parser.add_argument("--public", type=Path, required=True)
    args = parser.parse_args()
    generate_key_pair(args.private, args.public)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
