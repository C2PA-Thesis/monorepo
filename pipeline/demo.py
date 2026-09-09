#!/usr/bin/env python3
from __future__ import annotations

import argparse
import base64
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

from lib.presentation import Console, StepFailed
from lib.workflow import build_stages, runtime_env


ROOT = Path(__file__).resolve().parents[1]
PIPELINE = ROOT / "pipeline"
GENERATED = PIPELINE / "generated"
OUT = PIPELINE / "out"


def selected_photo() -> Path:
    value = os.environ.get("DEMO_PHOTO")
    photo = (Path(value).expanduser() if value else ROOT / "fixtures/generated/demo-input.jpg").resolve()
    if photo == OUT.resolve() or OUT.resolve() in photo.parents:
        raise ValueError("DEMO_PHOTO is inside pipeline/out, which this run replaces. Copy the input photo elsewhere first.")
    if value and not photo.is_file():
        raise ValueError("The selected photo does not exist: {}".format(photo))
    return photo


def run(console: Console) -> None:
    photo = selected_photo()
    skip_setup = os.environ.get("PIPELINE_SKIP_SETUP") == "1"
    console.line("=" * console.width, "36")
    console.line("  ONE PHOTO. TWO PROOFS. ONE SIGNED CAPTURE.", "1;36")
    console.detail("Capture -> crop -> location proof -> image proof -> publish -> verify -> test")
    console.line("=" * console.width, "36")
    console.field("Image selection", "DEMO_PHOTO override" if os.environ.get("DEMO_PHOTO") else "Default C2PA SDK sample photo")
    console.field("Selected input path", photo)
    if not os.environ.get("DEMO_PHOTO"):
        if skip_setup:
            console.detail("Use the existing local fixture. This run will not download another image.")
        else:
            console.detail("Setup downloads cli/sample/image.jpg from contentauth/c2pa-rs on GitHub.")
            console.detail("Setup copies that sample from c2pa/fixtures/image.jpg to the selected input path.")
    console.detail("This demo does not open a camera or search your folders for a photo.")
    console.field("Demo location", "UTDT, Buenos Aires | latitude -34.5478 | longitude -58.4462")
    console.detail("The coordinates are fixed test inputs. This run does not read GPS metadata from the photo.")
    console.field("Output directory to replace", OUT)
    console.detail("Existing run outputs in this directory will be removed after input checks pass.")

    run_id = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ") + "-" + str(os.getpid())
    logs = GENERATED / "logs" / run_id
    env = dict(os.environ, PIPELINE_EXPLAIN="1", PYTHONUNBUFFERED="1")
    env["PATH"] = env.get("C2PA_BIN_DIR", str(Path.home() / ".local/bin")) + os.pathsep + env.get("PATH", "")
    console.field("Logs for this run", logs)
    console.detail("Each log contains the exact command and full tool output. Use --verbose to show them here too.")
    console.detail("Press Ctrl+C to stop the current command.")
    console.line()
    console.line("  PREPARE THE TOOLS", "1;36")
    if skip_setup:
        console.detail("Reuse the Python environment, proof tools, keys, and proof parameters already on this computer.")
    else:
        console.detail("Install dependencies, build the proof tools, and create or reuse local test keys and parameters.")
        console.run("Prepare tools", [PIPELINE / "setup.sh"], logs / "setup.log", env, ROOT)

    env = runtime_env()
    python = PIPELINE / ".venv/bin/python"
    console.preview(photo, python, "Selected source image")
    region = ROOT / "fixtures/region-utdt-res7.json"
    region_data = json.loads(region.read_text())
    console.field("Public region", "H3 cell {} | resolution {} | {}".format(region_data["cell"], region_data["resolution"], region))
    console.detail("H3 divides the map into cells. This cell is the area the location check will request.")
    if OUT.is_symlink() or OUT.resolve() != OUT:
        raise ValueError("The output directory must not be a symbolic link: {}".format(OUT))
    if OUT.exists():
        shutil.rmtree(OUT)
    OUT.mkdir()
    console.detail("Input checks passed. The output directory is now ready for this run.")
    timings = OUT / "timings.tsv"
    timings.write_text("operation\twall_seconds\n")
    results = []

    def stage(number, label, title, explanation, command, outputs):
        console.stage(number, title, explanation)
        elapsed = console.run(title, command, logs / (label + ".log"), env, ROOT)
        results.append((title, elapsed))
        with timings.open("a") as file:
            file.write("{}\t{:.6f}\n".format(label, elapsed))
        for description, path in outputs:
            console.field(description, path)

    for item in build_stages(photo, -34.5478, -58.4462, region, OUT, env):
        stage(item.number, item.label, item.title, item.explanation, item.command, item.outputs)
        if item.number == 1:
            console.preview(OUT / "capture/original.png", python, "Prepared original: 1024 x 512")
        elif item.number == 2:
            console.preview(OUT / "edit/edited.png", python, "The crop that will be published")
        elif item.number == 3:
            console.detail("The known location coordinate-constraint gap remains.")
            console.detail("Next: the image prover can take a few minutes. It does not report internal phases or percent complete.")

    console.line()
    console.line("  [========]  8/8  ALL DEMO STAGES PASSED", "1;32")
    for title, seconds in results:
        if console.width >= 60:
            console.line("  {:<45} {:>8.2f}s".format(title, seconds))
        else:
            console.detail("{}: {:.2f}s".format(title, seconds))
    image_metadata = json.loads((OUT / "pi-img/metadata.json").read_text())
    image_size = sum(item["bytes"] for item in image_metadata["files"].values())
    location_size = len(base64.b64decode(json.loads((OUT / "pi-loc.json").read_text())["proof"]))
    console.field("Proof sizes", "HyperVerITAS: {:,} bytes | ZKLP: {:,} bytes".format(image_size, location_size))
    manifest_size = re.search(r"Manifest store size\s*=\s*(\d+)", (OUT / "manifest-store.txt").read_text())
    console.field("C2PA manifest store", "{:,} bytes".format(int(manifest_size.group(1)))
                  if manifest_size else "Size not reported by c2patool")
    console.field("Published PNG size", "{:,} bytes".format((OUT / "signed.png").stat().st_size))
    console.field("Open this image to see the result", OUT / "signed.png")
    console.field("Open this JSON to read its evidence", OUT / "manifest.json")
    console.field("Command timings", timings)
    console.field("Tool versions recorded during setup", GENERATED / "tool-versions.json")
    console.field("Complete logs", logs)
    console.detail("Share signed.png. The capture and negative-test directories also contain private inputs.")
    console.detail("These results show working demo checks. Location soundness and complete image privacy remain research work.")


def main() -> int:
    parser = argparse.ArgumentParser(prog="./pipeline/run.sh", description="Watch the complete C2PA proof demo, one explained step at a time.")
    parser.add_argument("--plain", action="store_true", help="Use static text without colors or animation.")
    parser.add_argument("--verbose", action="store_true", help="Also show the exact commands and raw tool output.")
    args = parser.parse_args()
    console = Console(args.plain, args.verbose)
    try:
        run(console)
    except KeyboardInterrupt:
        console.line("\n  [STOPPED] The current command was stopped. No later stage will run.", "1;33")
        return 130
    except StepFailed as error:
        console.line("  " + str(error), "1;31")
        return error.returncode
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        console.line("\n  [STOPPED] " + str(error), "1;31")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
