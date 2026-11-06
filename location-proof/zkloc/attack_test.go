package zkloc

import (
	"bytes"
	"encoding/base64"
	"math/big"
	"path/filepath"
	"strings"
	"testing"

	"github.com/consensys/gnark-crypto/ecc"
	"github.com/consensys/gnark/backend/groth16"
	"github.com/consensys/gnark/frontend"
	"github.com/tumberger/zk-Location/loc2index32"
)

// Security analysis, attack A1. Before the hints were sealed, a prover holding
// the secrets kept the honest envelope and fed the circuit the hints of another
// coordinate. These tests run that forgery with the real proving key, skipping
// the honest prover's own refusals.

var (
	honestSecrets = Secrets{
		Coordinate: Coordinate{Latitude: -34.5478, Longitude: -58.4462},
		Salt:       "0x" + strings.Repeat("0", 63) + "1",
	}
	// Tokyo Station, about 18,000 km from the honest coordinate in Buenos Aires.
	tokyo = Coordinate{Latitude: 35.6812, Longitude: 139.7671}
)

func TestAttackForgedHintsNoLongerProveAgainstTheSealedEnvelope(t *testing.T) {
	params := t.TempDir()
	if _, err := Setup(params); err != nil {
		t.Fatal(err)
	}
	lat, lng, salt, hints, err := honestSecrets.parse()
	if err != nil {
		t.Fatal(err)
	}
	envelope, err := loc2index32.NativeEnvelope(lat, lng, salt, hints)
	if err != nil {
		t.Fatal(err)
	}
	target, forged := tokyoTarget(t)

	_, err = proveUnchecked(params, target, lat, lng, salt, envelope, forged)
	if err == nil {
		t.Fatal("Tokyo's hints proved Tokyo's cell against the honest envelope")
	}
	if !strings.Contains(err.Error(), "not satisfied") {
		t.Fatalf("refused for another reason: %v", err)
	}
}

// If this proved, the capture point could sit outside the cell the reader is
// shown. The honest coordinate, hints and envelope go to the circuit with the
// neighboring cell, skipping the prover's own inside check.
func TestCoordinateOutsideTheCellDoesNotProve(t *testing.T) {
	params := t.TempDir()
	if _, err := Setup(params); err != nil {
		t.Fatal(err)
	}
	lat, lng, salt, hints, err := honestSecrets.parse()
	if err != nil {
		t.Fatal(err)
	}
	envelope, err := loc2index32.NativeEnvelope(lat, lng, salt, hints)
	if err != nil {
		t.Fatal(err)
	}
	neighbor, err := RegionOf("87c2e3021ffffff")
	if err != nil {
		t.Fatal(err)
	}

	_, err = proveUnchecked(params, neighbor, lat, lng, salt, envelope, hints)
	if err == nil {
		t.Fatal("a coordinate outside the cell proved it")
	}
	if !strings.Contains(err.Error(), "not satisfied") {
		t.Fatalf("refused for another reason: %v", err)
	}
}

// Passes while the circuit trusts whoever seals the envelope: hints of another
// coordinate, sealed into a new envelope, prove and verify for that cell. The
// device signature on the envelope is what refuses it, outside the circuit.
func TestAttackHintsSealedByTheSignerStillProveAnotherCell(t *testing.T) {
	params := t.TempDir()
	if _, err := Setup(params); err != nil {
		t.Fatal(err)
	}
	lat, lng, salt, _, err := honestSecrets.parse()
	if err != nil {
		t.Fatal(err)
	}
	target, forged := tokyoTarget(t)
	envelope, err := loc2index32.NativeEnvelope(lat, lng, salt, forged)
	if err != nil {
		t.Fatal(err)
	}

	proof, err := proveUnchecked(params, target, lat, lng, salt, envelope, forged)
	if err != nil {
		t.Fatalf("the sealed forgery did not prove: %v", err)
	}
	if err := Verify(params, target.Cell, formatScalar(envelope), proof); err != nil {
		t.Fatalf("the verifier rejected the sealed forgery: %v", err)
	}
}

func tokyoTarget(t *testing.T) (Region, [8]uint32) {
	t.Helper()
	target, err := CellOf(tokyo, 7)
	if err != nil {
		t.Fatal(err)
	}
	hints, err := HintsOf(tokyo)
	if err != nil {
		t.Fatal(err)
	}
	return target, hints.Hints
}

// proveUnchecked is Prove without checkHints and checkInside.
func proveUnchecked(dir string, r Region, lat, lng uint32, salt, envelope *big.Int, hints [8]uint32) (string, error) {
	circuit := groth16.NewCS(ecc.BN254)
	if err := readArtifact(filepath.Join(dir, CircuitFile), circuit); err != nil {
		return "", err
	}
	provingKey := groth16.NewProvingKey(ecc.BN254)
	if err := readArtifact(filepath.Join(dir, ProvingKeyFile), provingKey); err != nil {
		return "", err
	}
	assignment := &loc2index32.Circuit{
		Lat: lat, Lng: lng, Salt: salt, Envelope: envelope,
		Resolution: r.Resolution, Face: r.Face, I: r.I, J: r.J, K: r.K,
	}
	for i, hint := range hints {
		assignment.Hints[i] = hint
	}
	witness, err := frontend.NewWitness(assignment, ecc.BN254.ScalarField())
	if err != nil {
		return "", err
	}
	proof, err := groth16.Prove(circuit, provingKey, witness)
	if err != nil {
		return "", err
	}
	var encoded bytes.Buffer
	if _, err := proof.WriteTo(&encoded); err != nil {
		return "", err
	}
	return base64.StdEncoding.EncodeToString(encoded.Bytes()), nil
}
