package main

import (
	"bytes"
	"encoding/json"
	"strings"
	"testing"
)

const demoCell = "87c2e3020ffffff"

func call(t *testing.T, stdin string, args ...string) (string, error) {
	t.Helper()
	var out bytes.Buffer
	err := run(args, strings.NewReader(stdin), &out)
	return out.String(), err
}

func TestRegionOfTheDemoCell(t *testing.T) {
	got, err := regionOf(demoCell)
	want := region{Cell: demoCell, Resolution: 7, Face: 8, I: 1816, J: 0, K: 736}
	if err != nil || got != want {
		t.Fatalf("regionOf(%s) = %+v, %v; want %+v", demoCell, got, err, want)
	}
	if _, err := regionOf(strings.ToUpper(demoCell)); err == nil {
		t.Fatal("accepted an uppercase cell")
	}
}

func TestProveAndVerify(t *testing.T) {
	params := t.TempDir()
	if _, err := call(t, "", "setup", "--params", params); err != nil {
		t.Fatal(err)
	}
	secrets := `{"latitude": -34.5478, "longitude": -58.4462, "salt": "0x` + strings.Repeat("0", 63) + `1"}`

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
	if verifyWith(demoCell, "0x"+strings.Repeat("0", 63)+"2") == nil {
		t.Fatal("verified against another envelope")
	}

	_, err = call(t, secrets, "prove", "--params", params, "--cell", "87c2e3021ffffff")
	if err == nil || !strings.Contains(err.Error(), "not in cell") {
		t.Fatalf("proving a neighboring cell gave %v", err)
	}
}
