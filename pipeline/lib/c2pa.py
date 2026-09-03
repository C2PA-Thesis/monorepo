from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path
from typing import Any, Dict


ASSERTION_LABEL = "edu.utdt.td8.zkloc"


def c2patool_bin() -> str:
    return os.environ.get("C2PATOOL", "c2patool")


def sign_asset(
    edited: Path,
    manifest_path: Path,
    output: Path,
    report_path: Path,
) -> None:
    for name in ("C2PA_PRIVATE_KEY", "C2PA_SIGN_CERT"):
        if not os.environ.get(name):
            raise ValueError("{} must contain PEM data".format(name))
    completed = subprocess.run(
        [
            c2patool_bin(),
            str(edited),
            "--manifest",
            str(manifest_path),
            "--output",
            str(output),
            "--force",
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    report_path.write_text(completed.stdout + completed.stderr, encoding="utf-8")


def read_manifest(asset: Path) -> Dict[str, Any]:
    completed = subprocess.run(
        [c2patool_bin(), str(asset)],
        check=True,
        capture_output=True,
        text=True,
    )
    value = json.loads(completed.stdout)
    if not isinstance(value, dict):
        raise ValueError("c2patool returned a non-object manifest")
    return value


def manifest_store_info(asset: Path) -> str:
    completed = subprocess.run(
        [c2patool_bin(), str(asset), "--info"],
        check=True,
        capture_output=True,
        text=True,
    )
    return completed.stdout + completed.stderr


def assertion_from_manifest(manifest: Dict[str, Any]) -> Dict[str, Any]:
    active_label = manifest.get("active_manifest")
    manifests = manifest.get("manifests", {})
    if active_label not in manifests:
        raise ValueError("active C2PA manifest is missing")
    assertions = manifests[active_label].get("assertions", [])
    matches = [item for item in assertions if item.get("label") == ASSERTION_LABEL]
    if len(matches) != 1 or not isinstance(matches[0].get("data"), dict):
        raise ValueError("expected exactly one {} assertion".format(ASSERTION_LABEL))
    return matches[0]["data"]
