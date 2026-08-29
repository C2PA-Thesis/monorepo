#!/usr/bin/env bash
# Signs an image with a custom assertion, reads the manifest back, and checks
# the assertion survived.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
export PATH="${C2PA_BIN_DIR:-$HOME/.local/bin}:$PATH"

IN="${1:-fixtures/image.jpg}"
MANIFEST="${2:-manifests/zkloc-placeholder.json}"
OUT="out/signed.jpg"
LABEL="edu.utdt.td8.zkloc"

# These take PEM contents, not paths. A path fails with "Invalid certification data".
export C2PA_PRIVATE_KEY="$(cat certs/es256_private.key)"
export C2PA_SIGN_CERT="$(cat certs/es256_certs.pem)"

mkdir -p out
rm -f "$OUT" out/read-back.json

echo "==> firmando $IN con $MANIFEST"
c2patool "$IN" -m "$MANIFEST" -o "$OUT" -f > out/sign.json

echo "==> leyendo el manifiesto de vuelta desde $OUT"
c2patool "$OUT" > out/read-back.json

echo "==> verificando que la aserción custom sobrevivió"
python3 - "$LABEL" <<'PY'
import json, sys
label = sys.argv[1]
d = json.load(open("out/read-back.json"))
state = d.get("validation_state")
am = d["manifests"][d["active_manifest"]]
labels = [a["label"] for a in am["assertions"]]
ours = [a for a in am["assertions"] if a["label"] == label]

print(f"    validation_state : {state}")
print(f"    assertions       : {', '.join(labels)}")
if not ours:
    print(f"    FALLO: no se encontró la aserción {label}")
    sys.exit(1)

data = ours[0]["data"]
print(f"    {label}:")
print(f"      schema_version : {data['schema_version']}")
print(f"      commitment.alg : {data['commitment']['alg']}")
print(f"      h3 cell        : {data['location_proof']['cell']} (res {data['location_proof']['resolution']})")

if state != "Valid":
    print(f"    FALLO: validation_state = {state}")
    sys.exit(1)
print("    OK: la aserción sobrevivió y el manifiesto valida")
PY

echo
echo "==> salidas en out/"
echo "    $OUT              imagen firmada"
echo "    out/read-back.json  manifiesto leído de vuelta"
echo
echo "==> paso manual pendiente: subir $OUT a https://contentcredentials.org/verify"
echo "    y anotar el resultado en el README."
