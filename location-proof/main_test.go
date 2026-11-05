package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/C2PA-Thesis/monorepo/location-proof/zkloc"
)

const (
	demoCell       = "87c2e3020ffffff"
	demoCoordinate = `"latitude": -34.5478, "longitude": -58.4462`
	saltOne        = `"salt": "0x0000000000000000000000000000000000000000000000000000000000000001"`
	// From `location-proof hints` and `commit`; web/test/envelope.test.ts pins
	// the same values for the TypeScript implementation.
	demoHints    = `[3198105316,3205573900,3197634690,1064596499,3205444996,3210356339,3204052702,1063217901]`
	demoEnvelope = "0x0c5fe416c2846000ee1e623e19158db8cb35dbb69617a5c99ea523839e75ed29"
	tokyoHints   = `[1050986775,1058361726,1050467460,1064546442,1076804503,1059412339,1064329731,1051727743]`
)

func call(t *testing.T, stdin string, args ...string) (string, error) {
	t.Helper()
	var out bytes.Buffer
	err := run(args, strings.NewReader(stdin), &out)
	return out.String(), err
}

func TestCellOfTheDemoCoordinate(t *testing.T) {
	coordinate := `{"latitude": -34.5478, "longitude": -58.4462}`
	output, err := call(t, coordinate, "cell", "--resolution", "7")
	if err != nil || !strings.Contains(output, demoCell) {
		t.Fatalf("cell gave %q, %v; want %s", output, err, demoCell)
	}
	if _, err := call(t, coordinate, "cell", "--resolution", "16"); err == nil {
		t.Fatal("accepted resolution 16")
	}
	if _, err := call(t, `{"latitude": 91, "longitude": 0}`, "cell", "--resolution", "7"); err == nil {
		t.Fatal("accepted a latitude out of range")
	}
}

func TestOutlineOfTheDemoCell(t *testing.T) {
	output, err := call(t, "", "outline", "--cell", demoCell)
	if err != nil {
		t.Fatal(err)
	}
	var got zkloc.Outline
	if err := json.Unmarshal([]byte(output), &got); err != nil {
		t.Fatal(err)
	}
	if got.Cell != demoCell || got.Resolution != 7 || len(got.Boundary) != 6 {
		t.Fatalf("outline gave %+v; want the six corners of %s", got, demoCell)
	}
	if got.AreaKm2 < 1 || got.AreaKm2 > 10 {
		t.Fatalf("area %.2f km² is not the size of a resolution 7 cell", got.AreaKm2)
	}
	for _, corner := range got.Boundary {
		if corner[0] > -34.5 || corner[0] < -34.6 || corner[1] > -58.4 || corner[1] < -58.5 {
			t.Fatalf("corner %v is not near the demo coordinate", corner)
		}
	}
}

func TestHintsOfTheDemoCoordinate(t *testing.T) {
	output, err := call(t, "{"+demoCoordinate+"}", "hints")
	if err != nil || strings.TrimSpace(output) != `{"hints":`+demoHints+`}` {
		t.Fatalf("hints gave %q, %v; want %s", output, err, demoHints)
	}
	if _, err := call(t, `{"latitude": 91, "longitude": 0}`, "hints"); err == nil {
		t.Fatal("accepted a latitude out of range")
	}
}

func TestCommitSealsTheHints(t *testing.T) {
	commit := func(extra string) (string, error) {
		return call(t, "{"+demoCoordinate+", "+saltOne+extra+"}", "commit")
	}
	for _, extra := range []string{"", `, "hints": ` + demoHints} {
		output, err := commit(extra)
		if err != nil || !strings.Contains(output, demoEnvelope) {
			t.Fatalf("commit with %q gave %q, %v; want %s", extra, output, err, demoEnvelope)
		}
	}
	// One ulp off is still accepted, so this shows the hints are sealed.
	output, err := commit(`, "hints": ` + strings.Replace(demoHints, "3198105316", "3198105317", 1))
	if err != nil || strings.Contains(output, demoEnvelope) {
		t.Fatalf("commit with a hint one ulp off gave %q, %v; want another envelope", output, err)
	}
	if _, err := commit(`, "hints": ` + tokyoHints); err == nil || !strings.Contains(err.Error(), "another coordinate") {
		t.Fatalf("commit with another coordinate's hints gave %v", err)
	}
	for _, hints := range []string{`[]`, `[1, 2, 3, 4, 5, 6, 7]`, `[1, 2, 3, 4, 5, 6, 7, 8, 9]`, `[-1, 2, 3, 4, 5, 6, 7, 8]`, `[4294967296, 2, 3, 4, 5, 6, 7, 8]`} {
		if _, err := commit(`, "hints": ` + hints); err == nil {
			t.Fatalf("commit accepted hints %s", hints)
		}
	}
}

func TestProveAndVerify(t *testing.T) {
	params := t.TempDir()
	if _, err := call(t, "", "setup", "--params", params); err != nil {
		t.Fatal(err)
	}
	if version, err := os.ReadFile(filepath.Join(params, zkloc.VersionFile)); err != nil || string(version) != zkloc.CircuitVersion {
		t.Fatalf("setup wrote version %q, %v; want %q", version, err, zkloc.CircuitVersion)
	}
	secrets := "{" + demoCoordinate + ", " + saltOne + ", \"hints\": " + demoHints + "}"

	output, err := call(t, secrets, "prove", "--params", params, "--cell", demoCell)
	if err != nil {
		t.Fatal(err)
	}
	var proved struct{ Envelope, Proof string }
	if err := json.Unmarshal([]byte(output), &proved); err != nil {
		t.Fatal(err)
	}
	committed, err := call(t, secrets, "commit")
	if err != nil || !strings.Contains(committed, proved.Envelope) {
		t.Fatalf("commit gave %q, %v; prove gave envelope %s", committed, err, proved.Envelope)
	}

	verifyWith := func(cell, envelope string) error {
		_, err := call(t, "", "verify", "--params", params, "--cell", cell, "--envelope", envelope, "--proof", proved.Proof)
		return err
	}
	if err := verifyWith(demoCell, proved.Envelope); err != nil {
		t.Fatal(err)
	}
	if proved.Envelope != demoEnvelope {
		t.Fatalf("prove sealed envelope %s, want %s", proved.Envelope, demoEnvelope)
	}
	if verifyWith(demoCell, "0x"+strings.Repeat("0", 63)+"2") == nil {
		t.Fatal("verified against another envelope")
	}

	_, err = call(t, secrets, "prove", "--params", params, "--cell", "87c2e3021ffffff")
	if err == nil || !strings.Contains(err.Error(), "not in cell") {
		t.Fatalf("proving a neighboring cell gave %v", err)
	}

	forged := "{" + demoCoordinate + ", " + saltOne + ", \"hints\": " + tokyoHints + "}"
	_, err = call(t, forged, "prove", "--params", params, "--cell", demoCell)
	if err == nil || !strings.Contains(err.Error(), "another coordinate") {
		t.Fatalf("proving with another coordinate's hints gave %v", err)
	}

	for _, version := range []string{"", "sealed-hints-v0"} {
		path := filepath.Join(params, zkloc.VersionFile)
		if err := os.RemoveAll(path); err != nil {
			t.Fatal(err)
		}
		if version != "" {
			if err := os.WriteFile(path, []byte(version), 0o644); err != nil {
				t.Fatal(err)
			}
		}
		if _, err := call(t, secrets, "prove", "--params", params, "--cell", demoCell); err == nil || !strings.Contains(err.Error(), "run provenance setup again") {
			t.Fatalf("proving with version %q gave %v", version, err)
		}
		if err := verifyWith(demoCell, proved.Envelope); err == nil || !strings.Contains(err.Error(), "run provenance setup again") {
			t.Fatalf("verifying with version %q gave %v", version, err)
		}
	}

	// The params are now stale, as after a circuit change: setup replaces them
	// and then refuses to replace the current ones.
	if _, err := call(t, "", "setup", "--params", params); err != nil {
		t.Fatalf("setup over stale params gave %v", err)
	}
	if version, err := os.ReadFile(filepath.Join(params, zkloc.VersionFile)); err != nil || string(version) != zkloc.CircuitVersion {
		t.Fatalf("setup over stale params wrote version %q, %v", version, err)
	}
	if _, err := call(t, secrets, "prove", "--params", params, "--cell", demoCell); err != nil {
		t.Fatalf("proving with the replaced params gave %v", err)
	}
	if _, err := call(t, "", "setup", "--params", params); err == nil || !strings.Contains(err.Error(), "already holds") {
		t.Fatalf("setup over current params gave %v", err)
	}
}
