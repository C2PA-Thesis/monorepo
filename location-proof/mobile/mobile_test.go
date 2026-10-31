package main

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/C2PA-Thesis/monorepo/location-proof/zkloc"
)

// The same values location-proof's main_test.go pins for the CLI.
const (
	demoCell     = "87c2e3020ffffff"
	demoHints    = `{"hints":[3198105316,3205573900,3197634690,1064596499,3205444996,3210356339,3204052702,1063217901]}`
	demoEnvelope = `{"envelope":"0x0c5fe416c2846000ee1e623e19158db8cb35dbb69617a5c99ea523839e75ed29"}`
	demoSecrets  = `{"latitude": -34.5478, "longitude": -58.4462, "salt": "0x0000000000000000000000000000000000000000000000000000000000000001"}`
)

func text(value string) *string { return &value }

func errorOf(t *testing.T, reply string) string {
	t.Helper()
	var failure struct{ Error string }
	if err := json.Unmarshal([]byte(reply), &failure); err != nil {
		t.Fatalf("reply %q is not JSON: %v", reply, err)
	}
	return failure.Error
}

func TestHintsAndEnvelopeMatchTheCLI(t *testing.T) {
	if got := hintsReply(-34.5478, -58.4462); got != demoHints {
		t.Fatalf("hints gave %s, want %s", got, demoHints)
	}
	if got := envelopeReply(text(demoSecrets)); got != demoEnvelope {
		t.Fatalf("envelope gave %s, want %s", got, demoEnvelope)
	}
	var hints struct{ Hints []uint32 }
	if err := json.Unmarshal([]byte(demoHints), &hints); err != nil {
		t.Fatal(err)
	}
	encoded, _ := json.Marshal(hints.Hints)
	withHints := strings.TrimSuffix(demoSecrets, "}") + `, "hints": ` + string(encoded) + "}"
	if got := envelopeReply(text(withHints)); got != demoEnvelope {
		t.Fatalf("envelope with explicit hints gave %s, want %s", got, demoEnvelope)
	}
}

func TestCell(t *testing.T) {
	var got zkloc.Region
	if err := json.Unmarshal([]byte(cellReply(-34.5478, -58.4462, 7)), &got); err != nil || got.Cell != demoCell {
		t.Fatalf("cell gave %+v, %v; want %s", got, err, demoCell)
	}
}

func TestErrorsAreJSON(t *testing.T) {
	for name, reply := range map[string]string{
		"latitude out of range": hintsReply(91, 0),
		"resolution 16":         cellReply(-34.5478, -58.4462, 16),
		"NULL secrets":          envelopeReply(nil),
		"not JSON":              envelopeReply(text("{")),
		"unknown field":         envelopeReply(text(`{"latitude": 0, "longitude": 0, "salt": "0x01", "x": 1}`)),
		"missing coordinate":    envelopeReply(text(`{"salt": "0x0000000000000000000000000000000000000000000000000000000000000001"}`)),
		"trailing data":         envelopeReply(text(demoSecrets + " trailing")),
		"NULL params":           proveReply(nil, text(demoCell), text(demoSecrets)),
		"missing params":        proveReply(text(t.TempDir()), text(demoCell), text(demoSecrets)),
	} {
		if message := errorOf(t, reply); message == "" {
			t.Errorf("%s: reply %s has no error", name, reply)
		}
	}
	if message := errorOf(t, respond(func() (any, error) { panic("boom") })); message != "internal error: boom" {
		t.Fatalf("panic gave %q", message)
	}
}

func TestProveVerifies(t *testing.T) {
	if testing.Short() {
		t.Skip("runs a Groth16 setup")
	}
	params := t.TempDir()
	if _, err := zkloc.Setup(params); err != nil {
		t.Fatal(err)
	}
	var proof zkloc.Proof
	reply := proveReply(text(params), text(demoCell), text(demoSecrets))
	if err := json.Unmarshal([]byte(reply), &proof); err != nil || proof.Proof == "" {
		t.Fatalf("prove gave %s, %v", reply, err)
	}
	var want zkloc.Envelope
	if err := json.Unmarshal([]byte(demoEnvelope), &want); err != nil {
		t.Fatal(err)
	}
	if proof.Envelope != want.Envelope {
		t.Fatalf("prove sealed %q, want %q", proof.Envelope, want.Envelope)
	}
	if err := zkloc.Verify(params, demoCell, proof.Envelope, proof.Proof); err != nil {
		t.Fatal(err)
	}
}
