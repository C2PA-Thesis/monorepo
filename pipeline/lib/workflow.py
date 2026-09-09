from __future__ import annotations

import os
import shutil
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
PIPELINE = ROOT / "pipeline"
GENERATED = PIPELINE / "generated"


@dataclass(frozen=True)
class Stage:
    number: int
    label: str
    title: str
    explanation: str
    command: list
    outputs: list


def runtime_env() -> dict:
    env = dict(os.environ, PIPELINE_EXPLAIN="1", PYTHONUNBUFFERED="1")
    env["PATH"] = env.get("C2PA_BIN_DIR", str(Path.home() / ".local/bin")) + os.pathsep + env.get("PATH", "")
    env["HV_BIN"] = env.get("HV_BIN", str(ROOT / "editproof/hyperveritas_impl/target/release/hv"))
    env["ZKLOC_BIN"] = env.get("ZKLOC_BIN", str(GENERATED / "bin/zkloc"))
    for binary in [env["HV_BIN"], env["ZKLOC_BIN"], env.get("C2PATOOL", "c2patool")]:
        if not shutil.which(binary, path=env["PATH"]):
            raise ValueError("Proof tools are not ready. Run ./pipeline/setup.sh first. Missing: " + binary)
    for name, filename in [("C2PA_PRIVATE_KEY", "es256_private.key"), ("C2PA_SIGN_CERT", "es256_certs.pem")]:
        env[name] = (ROOT / "c2pa/certs" / filename).read_text()
    for file in [ROOT / "fixtures/generated/device-private.pem", ROOT / "fixtures/generated/device-public.pem",
                 GENERATED / "hv-pst/metadata.json", GENERATED / "zkloc-groth16/metadata.json", PIPELINE / ".venv/bin/python"]:
        if not file.is_file():
            raise ValueError("Setup file missing. Run ./pipeline/setup.sh first: " + str(file))
    return env


def build_stages(photo: Path, latitude: float, longitude: float, region: Path, out: Path, env: dict) -> list:
    python = PIPELINE / ".venv/bin/python"
    hv, zkloc = env["HV_BIN"], env["ZKLOC_BIN"]
    device_private = ROOT / "fixtures/generated/device-private.pem"
    device_public = ROOT / "fixtures/generated/device-public.pem"
    OUT = out
    result = []

    def stage(*args):
        result.append(Stage(*args))

    stage(
        1, "capture", "Create the capture receipt",
        "Create two public values: an image fingerprint and a salted location commitment "
        "(the envelope). Sign them together in a capture receipt.",
        [
            python, PIPELINE / "capture.py", photo,
            "--lat", str(latitude), "--lon", str(longitude),
            "--device-key", device_private, "--out", OUT / "capture",
        ],
        [
            ("Private original pixels", OUT / "capture/original.png"),
            ("Private coordinates and salt", OUT / "capture/secrets.json"),
            ("Public signed receipt", OUT / "capture/receipt.json"),
        ],
    )

    stage(
        2, "crop", "Keep the left half of the photo",
        "[ LEFT HALF: keep ] [ RIGHT HALF: remove ]. "
        "Save a 512 x 512 PNG without changing the kept pixels.",
        [python, PIPELINE / "crop.py", "--capture", OUT / "capture", "--out", OUT / "edit"],
        [
            ("Edited image", OUT / "edit/edited.png"),
            ("Edited pixel data for the proof", OUT / "edit/edited.json"),
        ],
    )

    stage(
        3, "location-prover", "Build the location proof with ZKLP",
        "Read the private coordinates and salt. Build a Groth16 proof "
        "for the receipt envelope and the requested H3 region.",
        [
            zkloc, "prove", "--params", GENERATED / "zkloc-groth16",
            "--secrets", OUT / "capture/secrets.json",
            "--region", region, "--out", OUT / "pi-loc.json",
        ],
        [("Location proof", OUT / "pi-loc.json")],
    )

    stage(
        4, "image-prover", "Build the crop proof with HyperVerITAS",
        "Read the original, crop, and signed fingerprint. Verify the inputs, "
        "build a PST proof, then verify it before saving.",
        [
            hv, "prove-crop", "--params", GENERATED / "hv-pst",
            "--original", OUT / "capture/original.json",
            "--edited", OUT / "edit/edited.json",
            "--fingerprint", OUT / "capture/fingerprint.json", "--out", OUT / "pi-img",
        ],
        [("Image proof", OUT / "pi-img/proof.bin")],
    )

    stage(
        5, "c2pa-package", "Put the evidence inside the published PNG",
        "C2PA stores signed provenance inside the image. Embed the receipt, region, "
        "and both proofs, then sign the PNG with the separate sample editor key.",
        [
            python, PIPELINE / "package.py", "--receipt", OUT / "capture/receipt.json",
            "--region", region, "--pi-loc", OUT / "pi-loc.json", "--pi-img", OUT / "pi-img",
            "--edited", OUT / "edit/edited.png", "--out", OUT / "signed.png",
        ],
        [
            ("Published image with embedded evidence", OUT / "signed.png"),
            ("Readable manifest", OUT / "manifest.json"),
        ],
    )

    stage(
        6, "verifier", "Verify the file as a reader",
        "Use the signed PNG, requested region, trusted device public key, and proof parameters. "
        "The reader does not load the original or secrets.",
        [
            python, PIPELINE / "verify.py", OUT / "signed.png", "--region", region,
            "--device-public-key", device_public,
            "--hv-params", GENERATED / "hv-pst", "--zkloc-vk", GENERATED / "zkloc-groth16",
        ],
        [],
    )

    stage(
        7, "receipt-verification-benchmark", "Measure the receipt check",
        "Repeat the signature check and the complete receipt check 100 times. Save the median times.",
        [
            python, PIPELINE / "benchmark_receipt.py", "--receipt", OUT / "capture/receipt.json",
            "--device-public-key", device_public, "--out", OUT / "receipt-verification.json",
        ],
        [("Receipt timings", OUT / "receipt-verification.json")],
    )

    stage(
        8, "binding-negative-integration", "Try six deliberate changes",
        "Reuse the real proofs. Change one part at a time and require the expected rejection. "
        "A rejection means the test passed.",
        [
            python, PIPELINE / "integration_negatives.py", "--valid", OUT,
            "--photo", photo, "--region", region,
            "--device-private-key", device_private, "--device-public-key", device_public,
            "--hv-params", GENERATED / "hv-pst", "--zkloc-params", GENERATED / "zkloc-groth16",
            "--out", OUT / "negative",
        ],
        [("Expected and observed test results", OUT / "negative/results.json")],
    )
    return result
