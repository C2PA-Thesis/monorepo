from __future__ import annotations

import base64
import contextlib
import hashlib
import io
import json
import shutil
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

from cryptography.hazmat.primitives import serialization
from cryptography.hazmat.primitives.asymmetric import ec
from PIL import Image

import package as package_command
import verify
from lib.c2pa import ASSERTION_LABEL
from lib.contracts import load_json
from lib.receipt import create_receipt

from .samples import ONE, ZERO, clone, image_proof, location_proof, region, signed_receipt


class NegativePackagedFlowTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.private_key = ec.generate_private_key(ec.SECP256R1())
        self.public_key_path = self.root / "device-public.pem"
        self.public_key_path.write_bytes(
            self.private_key.public_key().public_bytes(
                serialization.Encoding.PEM,
                serialization.PublicFormat.SubjectPublicKeyInfo,
            )
        )
        self.region = region()
        self.receipt = signed_receipt(self.private_key)
        self.pi_loc = location_proof(self.receipt["envelope"], self.region)
        self.proof_dir = self.root / "pi-img"
        self.image_proof = image_proof(self.proof_dir)
        self.edited = self.root / "edited.png"
        Image.new("RGB", (512, 512), (17, 34, 51)).save(self.edited, format="PNG")
        self.signed = self.root / "signed.png"
        self._package_with_stubbed_c2pa()
        manifest = load_json(self.root / "manifest.json")
        self.assertion = manifest["assertions"][1]["data"]
        self.expected_fingerprint = clone(self.receipt["fingerprint"])
        self.expected_pixel_digest = self._pixel_digest(self.signed)

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def _package_with_stubbed_c2pa(self) -> None:
        receipt_path = self.root / "receipt.json"
        region_path = self.root / "region.json"
        pi_loc_path = self.root / "pi-loc.json"
        for path, value in (
            (receipt_path, self.receipt),
            (region_path, self.region),
            (pi_loc_path, self.pi_loc),
        ):
            path.write_text(json.dumps(value), encoding="utf-8")

        def fake_sign(edited: Path, manifest: Path, output: Path, report: Path) -> None:
            shutil.copyfile(edited, output)
            report.write_text("stubbed c2patool\n", encoding="utf-8")

        with mock.patch.object(package_command, "sign_asset", side_effect=fake_sign), mock.patch.object(
            package_command, "manifest_store_info", return_value="stubbed manifest info\n"
        ):
            package_command.package_asset(
                receipt_path,
                region_path,
                pi_loc_path,
                self.proof_dir,
                self.edited,
                self.signed,
            )

    @staticmethod
    def _pixel_digest(path: Path) -> str:
        with Image.open(path) as image:
            return hashlib.sha256(image.convert("RGB").tobytes()).hexdigest()

    def _manifest(self, assertion: dict) -> dict:
        return {
            "validation_state": "Valid",
            "active_manifest": "urn:test",
            "manifests": {
                "urn:test": {
                    "assertions": [{"label": ASSERTION_LABEL, "data": assertion}]
                }
            },
        }

    def _proof_stub(self, command: list, check: int) -> None:
        if check == 2:
            fingerprint_path = Path(command[command.index("--fingerprint") + 1])
            edited_path = Path(command[command.index("--edited") + 1])
            if load_json(fingerprint_path) != self.expected_fingerprint:
                raise verify.CheckFailure(2)
            edited = load_json(edited_path)
            pixels = bytearray()
            for red, green, blue in zip(edited["R"], edited["G"], edited["B"]):
                pixels.extend((red, green, blue))
            if hashlib.sha256(bytes(pixels)).hexdigest() != self.expected_pixel_digest:
                raise verify.CheckFailure(2)
        if check == 3:
            envelope = command[command.index("--envelope") + 1]
            if envelope != ONE:
                raise verify.CheckFailure(3)

    def _verify(self, assertion: dict) -> None:
        region_path = self.root / "requested-region.json"
        region_path.write_text(json.dumps(self.region), encoding="utf-8")
        with mock.patch.object(
            verify, "read_manifest", return_value=self._manifest(assertion)
        ), mock.patch.object(verify, "_run_check", side_effect=self._proof_stub):
            verify.verify_asset(
                self.signed,
                region_path,
                self.public_key_path,
                self.root / "hv-params",
                self.root / "zkloc-vk",
            )

    def _assert_fails(self, assertion: dict, expected_check: int) -> None:
        with self.assertRaises(verify.CheckFailure) as context:
            self._verify(assertion)
        self.assertEqual(context.exception.check, expected_check)

    def test_valid_stubbed_packaged_flow_passes(self) -> None:
        self._verify(self.assertion)

    def test_mutated_receipt_signature_fails_check_1(self) -> None:
        assertion = clone(self.assertion)
        signature = bytearray(base64.b64decode(assertion["receipt"]["signature"]))
        signature[-1] ^= 1
        assertion["receipt"]["signature"] = base64.b64encode(signature).decode("ascii")
        self._assert_fails(assertion, 1)

    def test_unsigned_fingerprint_replacement_fails_check_1(self) -> None:
        assertion = clone(self.assertion)
        assertion["receipt"]["fingerprint"]["channels"]["R"][0] = ONE
        self._assert_fails(assertion, 1)

    def test_resigned_fingerprint_replacement_fails_check_2(self) -> None:
        assertion = clone(self.assertion)
        replacement = clone(self.receipt["fingerprint"])
        replacement["channels"]["R"][0] = ONE
        assertion["receipt"] = create_receipt(
            replacement,
            self.receipt["envelope"],
            self.receipt["captured_at"],
            self.private_key,
        )
        self._assert_fails(assertion, 2)

    def test_wrong_requested_region_fails_check_3(self) -> None:
        assertion = clone(self.assertion)
        assertion["region"]["cell"] = "87c2e3021ffffff"
        self._assert_fails(assertion, 3)

    def test_receipt_from_another_capture_fails_check_2(self) -> None:
        assertion = clone(self.assertion)
        assertion["receipt"] = signed_receipt(
            self.private_key,
            fingerprint_value=ONE,
            envelope_value="0x" + "00" * 31 + "02",
        )
        self._assert_fails(assertion, 2)

    def test_unproven_pixel_mutation_fails_check_2(self) -> None:
        with Image.open(self.signed) as image:
            rgb = image.convert("RGB")
            rgb.putpixel((0, 0), (99, 88, 77))
            rgb.save(self.signed, format="PNG")
        self._assert_fails(self.assertion, 2)

    def test_tampered_inline_proof_member_fails_check_2(self) -> None:
        assertion = clone(self.assertion)
        assertion["image_proof"]["members"]["proof.bin"]["data"] = "AA=="
        self._assert_fails(assertion, 2)

    def test_cli_prints_only_the_failed_check(self) -> None:
        arguments = SimpleNamespace(
            signed=self.signed,
            region=self.root / "region.json",
            device_public_key=self.public_key_path,
            hv_params=self.root / "hv-params",
            zkloc_vk=self.root / "zkloc-vk",
        )
        stderr = io.StringIO()
        with mock.patch.object(verify, "parse_args", return_value=arguments), mock.patch.object(
            verify, "verify_asset", side_effect=verify.CheckFailure(2)
        ), contextlib.redirect_stderr(stderr):
            self.assertEqual(verify.main(), 1)
        self.assertEqual(stderr.getvalue(), "check 2\n")


if __name__ == "__main__":
    unittest.main()
