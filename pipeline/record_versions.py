#!/usr/bin/env python3
from __future__ import annotations

import argparse
import importlib.metadata
import platform
import subprocess
import sys
from pathlib import Path

from lib.contracts import write_json


def command_version(command: list) -> str:
    completed = subprocess.run(command, check=True, capture_output=True, text=True)
    output = (completed.stdout + completed.stderr).strip()
    return output.splitlines()[0]


def git_revision(path: Path) -> str:
    return command_version(["git", "-C", str(path), "rev-parse", "HEAD"])


def main() -> int:
    parser = argparse.ArgumentParser(description="Record the exact PoC tool versions")
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--hv", required=True)
    parser.add_argument("--zkloc", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    versions = {
        "python": sys.version.splitlines()[0],
        "platform": platform.platform(),
        "Pillow": importlib.metadata.version("Pillow"),
        "cryptography": importlib.metadata.version("cryptography"),
        "jsonschema": importlib.metadata.version("jsonschema"),
        "h3": importlib.metadata.version("h3"),
        "c2patool": command_version(["c2patool", "--version"]),
        "hv": command_version([args.hv, "--version"]),
        "zkloc": command_version([args.zkloc, "--version"]),
        "go": command_version(["go", "version"]),
        "rustc": command_version(["rustc", "--version"]),
        "editproof_commit": git_revision(args.root / "editproof"),
        "locproof_commit": git_revision(args.root / "locproof"),
    }
    write_json(args.out, versions)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
