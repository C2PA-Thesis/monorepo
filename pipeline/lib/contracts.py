from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Dict

from jsonschema import Draft202012Validator, FormatChecker
from referencing import Registry, Resource


PIPELINE_ROOT = Path(__file__).resolve().parents[1]
SCHEMA_ROOT = PIPELINE_ROOT / "schemas"
BLS12_381_SCALAR_MODULUS = int(
    "73eda753299d7d483339d80809a1d80553bda402fffe5bfeffffffff00000001",
    16,
)
BN254_SCALAR_MODULUS = int(
    "30644e72e131a029b85045b68181585d2833e84879b9709143e1f593f0000001",
    16,
)


def load_json(path: Path) -> Dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError("expected a JSON object in {}".format(path))
    return value


def write_json(path: Path, value: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        json.dump(value, handle, indent=2, sort_keys=True)
        handle.write("\n")


def canonical_json(value: Dict[str, Any]) -> bytes:
    return json.dumps(
        value,
        ensure_ascii=False,
        separators=(",", ":"),
        sort_keys=True,
    ).encode("utf-8")


def _schemas() -> Dict[str, Dict[str, Any]]:
    result = {}
    for path in SCHEMA_ROOT.glob("*.schema.json"):
        schema = load_json(path)
        result[schema["$id"]] = schema
    return result


def validate(document: Dict[str, Any], schema_name: str) -> None:
    schemas = _schemas()
    schema_path = SCHEMA_ROOT / "{}.schema.json".format(schema_name)
    schema = load_json(schema_path)
    registry = Registry()
    for identifier, resource_schema in schemas.items():
        registry = registry.with_resource(
            identifier,
            Resource.from_contents(resource_schema),
        )
    Draft202012Validator(
        schema,
        registry=registry,
        format_checker=FormatChecker(),
    ).validate(document)
    _validate_field_ranges(document, schema_name)


def _require_scalar(value: str, modulus: int, name: str) -> None:
    if int(value[2:], 16) >= modulus:
        raise ValueError("{} is not a canonical field element".format(name))


def _validate_field_ranges(document: Dict[str, Any], schema_name: str) -> None:
    if schema_name == "fingerprint":
        for channel_name, values in document["channels"].items():
            for index, value in enumerate(values):
                _require_scalar(
                    value,
                    BLS12_381_SCALAR_MODULUS,
                    "fingerprint.{}[{}]".format(channel_name, index),
                )
    elif schema_name == "envelope":
        _require_scalar(document["value"], BN254_SCALAR_MODULUS, "envelope.value")
    elif schema_name == "secrets":
        _require_scalar(document["salt"], BN254_SCALAR_MODULUS, "secrets.salt")
        if int(document["salt"][2:], 16) >= 2**248:
            raise ValueError("secrets.salt exceeds 248 bits")
    elif schema_name == "receipt":
        _validate_field_ranges(document["fingerprint"], "fingerprint")
        _validate_field_ranges(document["envelope"], "envelope")
    elif schema_name == "pi_loc":
        _require_scalar(
            document["public_inputs"]["envelope"],
            BN254_SCALAR_MODULUS,
            "pi_loc.public_inputs.envelope",
        )
    elif schema_name == "assertion":
        _validate_field_ranges(document["receipt"], "receipt")
        _validate_field_ranges(document["location_proof"], "pi_loc")
