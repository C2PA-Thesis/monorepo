package zkloc

import (
	"os"
	"path/filepath"
	"strings"
	"testing"
)

const demoCell = "87c2e3020ffffff"

func TestRegionOfTheDemoCell(t *testing.T) {
	got, err := RegionOf(demoCell)
	want := Region{Cell: demoCell, Resolution: 7, Face: 8, I: 1816, J: 0, K: 736}
	if err != nil || got != want {
		t.Fatalf("RegionOf(%s) = %+v, %v; want %+v", demoCell, got, err, want)
	}
	if _, err := RegionOf(strings.ToUpper(demoCell)); err == nil {
		t.Fatal("accepted an uppercase cell")
	}
}

func TestDecodeRefusesUnknownFields(t *testing.T) {
	var s Secrets
	if err := Decode(strings.NewReader(`{"latitude": 0, "longitude": 0, "salt": "0x01", "extra": 1}`), &s); err == nil {
		t.Fatal("accepted an unknown field")
	}
}

func TestDecodeRefusesMissingFieldsAndTrailingData(t *testing.T) {
	const salt = `"salt": "0x0000000000000000000000000000000000000000000000000000000000000001"`
	for _, text := range []string{
		`{` + salt + `}`,
		`{"latitude": -34.5, ` + salt + `}`,
		`{"latitude": -34.5, "longitude": null, ` + salt + `}`,
		`{"latitude": -34.5, "longitude": -58.4}`,
		`{"latitude": -34.5, "longitude": -58.4, ` + salt + `} trailing`,
		`{"latitude": -34.5, "longitude": -58.4, ` + salt + `} {}`,
		`[]`,
	} {
		var s Secrets
		if err := Decode(strings.NewReader(text), &s); err == nil {
			t.Errorf("accepted secrets %s", text)
		}
	}
	var c Coordinate
	if err := Decode(strings.NewReader(`{"latitude": -34.5}`), &c); err == nil {
		t.Error("accepted a coordinate without a longitude")
	}
	var s Secrets
	if err := Decode(strings.NewReader(`{"latitude": 0, "longitude": 0, `+salt+`, "hints": null}`+"\n"), &s); err != nil {
		t.Fatalf("refused complete secrets: %v", err)
	}
}

func TestSetupRefusesToReplaceACurrentSetup(t *testing.T) {
	dir := t.TempDir()
	for name, content := range map[string]string{VersionFile: CircuitVersion, VerifyingKeyFile: "kept"} {
		if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := Setup(dir); err == nil || !strings.Contains(err.Error(), "already holds") {
		t.Fatalf("Setup over a current setup gave %v", err)
	}
	if kept, err := os.ReadFile(filepath.Join(dir, VerifyingKeyFile)); err != nil || string(kept) != "kept" {
		t.Fatalf("Setup touched the current verifying key: %q, %v", kept, err)
	}
}

func TestUlpsApart(t *testing.T) {
	for _, c := range []struct {
		a, b uint32
		want uint64
	}{
		{0x3f800000, 0x3f800000, 0}, {0x3f800000, 0x3f800001, 1}, {0x80000000, 0x00000000, 0},
		{0x80000001, 0x00000001, 2}, {0xbf800000, 0x3f800000, 2 * 0x3f800000},
	} {
		if got := ulpsApart(c.a, c.b); got != c.want {
			t.Fatalf("ulpsApart(%#x, %#x) = %d, want %d", c.a, c.b, got, c.want)
		}
	}
}
