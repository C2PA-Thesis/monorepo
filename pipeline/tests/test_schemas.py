from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

import h3
from cryptography.hazmat.primitives.asymmetric import ec
from jsonschema import ValidationError

from lib.contracts import load_json, validate
from integration_negatives import different_fingerprint
from package import build_assertion

from .samples import envelope, fingerprint, image_proof, location_proof, region, signed_receipt


class SchemaTests(unittest.TestCase):
    def test_public_demo_point_maps_to_the_tracked_h3_cell(self) -> None:
        self.assertEqual(
            h3.latlng_to_cell(-34.5478, -58.4462, 7),
            "87c2e3020ffffff",
        )

    def test_tracked_c2pa_example_uses_the_assertion_schema(self) -> None:
        root = Path(__file__).resolve().parents[2]
        manifest = load_json(root / "c2pa" / "manifests" / "zkloc-example.json")
        validate(manifest["assertions"][1]["data"], "assertion")

    def test_every_contract_accepts_a_version_one_fixture(self) -> None:
        key = ec.generate_private_key(ec.SECP256R1())
        receipt = signed_receipt(key)
        public_region = region()
        with tempfile.TemporaryDirectory() as temporary:
            bundled_image_proof = image_proof(Path(temporary) / "proof")
            assertion = build_assertion(
                receipt,
                public_region,
                location_proof(receipt["envelope"], public_region),
                bundled_image_proof,
            )

            fixtures = {
                "fingerprint": fingerprint(),
                "envelope": envelope(),
                "receipt": receipt,
                "secrets": {
                    "schema_version": 1,
                    "image_path": "original.json",
                    "latitude_degrees": -34.5478,
                    "longitude_degrees": -58.4462,
                    "salt": "0x00" + "11" * 31,
                    "captured_at": "2026-09-02T12:00:00Z",
                },
                "region": public_region,
                "pi_loc": location_proof(receipt["envelope"], public_region),
                "pi_img-metadata": bundled_image_proof["metadata"],
                "pi_img-bundle": bundled_image_proof,
                "assertion": assertion,
            }
            for name, fixture in fixtures.items():
                with self.subTest(schema=name):
                    validate(fixture, name)

    def test_region_requires_a_global_face(self) -> None:
        fixture = region()
        del fixture["face"]
        with self.assertRaises(ValidationError):
            validate(fixture, "region")

    def test_fingerprint_requires_exactly_128_elements_per_channel(self) -> None:
        fixture = fingerprint()
        fixture["channels"]["R"].pop()
        with self.assertRaises(ValidationError):
            validate(fixture, "fingerprint")

    def test_negative_runner_uses_a_different_canonical_fingerprint(self) -> None:
        original = fingerprint()
        changed = different_fingerprint(original)
        self.assertNotEqual(changed, original)
        validate(changed, "fingerprint")

    def test_receipt_rejects_an_embedded_public_key(self) -> None:
        fixture = signed_receipt(ec.generate_private_key(ec.SECP256R1()))
        fixture["device_public_key"] = "not allowed"
        with self.assertRaises(ValidationError):
            validate(fixture, "receipt")

    def test_envelope_rejects_a_noncanonical_bn254_scalar(self) -> None:
        fixture = envelope(
            "0x30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001"
        )
        with self.assertRaisesRegex(ValueError, "canonical field element"):
            validate(fixture, "envelope")

    def test_envelope_rejects_an_unimplemented_coordinate_encoding(self) -> None:
        fixture = envelope()
        fixture["encoding"] = "f64-radians-ieee754-bits+salt-field"
        with self.assertRaises(ValidationError):
            validate(fixture, "envelope")


if __name__ == "__main__":
    unittest.main()
