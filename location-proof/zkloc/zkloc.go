// Package zkloc verifies, with the ZKLP FP32 circuit from the C2PA-Thesis
// fork of zk-Location, that the trigonometric hints sealed with a secret
// coordinate map to a public H3 cell.
//
// The circuit maps the coordinate to a cell through trigonometric hints that
// it does not derive from the coordinate. The hints are sealed in the
// envelope, so a prover cannot change them after the envelope is signed, but
// whoever computes the envelope chooses them. An accepted proof is as
// trustworthy as the signer of the envelope.
//
// The location-proof command and the iOS library in ./mobile are thin
// wrappers over this package, so both produce the same JSON.
package zkloc

import (
	"bytes"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"math/big"
	"os"
	"path/filepath"
	"strings"

	"github.com/consensys/gnark-crypto/ecc"
	"github.com/consensys/gnark-crypto/ecc/bn254/fr"
	"github.com/consensys/gnark/backend/groth16"
	"github.com/consensys/gnark/frontend"
	"github.com/consensys/gnark/frontend/cs/r1cs"
	"github.com/tumberger/zk-Location/loc2index32"
	h3 "github.com/uber/h3-go/v4"
)

const (
	CircuitFile      = "circuit.r1cs"
	ProvingKeyFile   = "proving.key"
	VerifyingKeyFile = "verifying.key"
	VersionFile      = "version"
	// CircuitVersion changes whenever the circuit does, so params made for an
	// older circuit are refused instead of failing inside the prover.
	CircuitVersion = "sealed-hints-v1"
)

type Coordinate struct {
	Latitude  float64 `json:"latitude"`
	Longitude float64 `json:"longitude"`
}

type Secrets struct {
	Coordinate
	Salt string `json:"salt"`
	// Hints are optional; without them the coordinate's own are used.
	Hints []uint32 `json:"hints,omitempty"`
}

// Outline is what a map needs to draw a cell: its corners and its area.
type Outline struct {
	Cell       string       `json:"cell"`
	Resolution int          `json:"resolution"`
	AreaKm2    float64      `json:"area_km2"`
	Boundary   [][2]float64 `json:"boundary"`
}

// Region is the public tuple the circuit checks the coordinate against.
type Region struct {
	Cell       string `json:"cell"`
	Resolution int    `json:"resolution"`
	Face       int    `json:"face"`
	I          int    `json:"i"`
	J          int    `json:"j"`
	K          int    `json:"k"`
}

type Hints struct {
	Hints [8]uint32 `json:"hints"`
}

type Envelope struct {
	Envelope string `json:"envelope"`
}

// Proof is a base64 Groth16 proof and the envelope it was made for.
type Proof struct {
	Envelope string `json:"envelope"`
	Proof    string `json:"proof"`
}

// Constraints is the size of the circuit a setup was made for.
type Constraints struct {
	Constraints int `json:"constraints"`
}

// Decode reads exactly one JSON value. It refuses fields the value does not
// have, trailing data, and required fields that are missing or null, so a
// caller bug cannot silently commit to (0, 0) or a zero salt.
func Decode(r io.Reader, value any) error {
	var raw json.RawMessage
	decoder := json.NewDecoder(r)
	if err := decoder.Decode(&raw); err != nil {
		return err
	}
	if _, err := decoder.Token(); !errors.Is(err, io.EOF) {
		return errors.New("unexpected data after the JSON value")
	}
	strict := json.NewDecoder(bytes.NewReader(raw))
	strict.DisallowUnknownFields()
	if err := strict.Decode(value); err != nil {
		return err
	}
	withRequired, ok := value.(interface{ requiredFields() []string })
	if !ok {
		return nil
	}
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(raw, &fields); err != nil {
		return err
	}
	for _, name := range withRequired.requiredFields() {
		if field, ok := fields[name]; !ok || string(field) == "null" {
			return fmt.Errorf("%q is required", name)
		}
	}
	return nil
}

func (Coordinate) requiredFields() []string { return []string{"latitude", "longitude"} }

func (Secrets) requiredFields() []string { return []string{"latitude", "longitude", "salt"} }

// Setup compiles the circuit and writes a fresh Groth16 setup to dir. It
// refuses to replace a complete setup for the current circuit, since that
// would orphan every proof made with it, but replaces one for an older
// circuit or one that was cut short.
func Setup(dir string) (Constraints, error) {
	if _, err := os.Stat(filepath.Join(dir, VerifyingKeyFile)); err == nil && checkVersion(dir) == nil {
		return Constraints{}, fmt.Errorf("%s already holds a %s setup; delete it to run setup again", dir, CircuitVersion)
	}
	// The verifying key goes first, so no mix of old and new files ever
	// looks like a complete setup.
	for _, name := range []string{VerifyingKeyFile, VersionFile, ProvingKeyFile, CircuitFile} {
		if err := os.Remove(filepath.Join(dir, name)); err != nil && !errors.Is(err, os.ErrNotExist) {
			return Constraints{}, err
		}
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return Constraints{}, err
	}
	circuit, err := frontend.Compile(ecc.BN254.ScalarField(), r1cs.NewBuilder, &loc2index32.Circuit{})
	if err != nil {
		return Constraints{}, fmt.Errorf("compiling the circuit: %w", err)
	}
	provingKey, verifyingKey, err := groth16.Setup(circuit)
	if err != nil {
		return Constraints{}, fmt.Errorf("groth16 setup: %w", err)
	}
	// The verifying key goes last: its presence marks a complete setup.
	for _, artifact := range []struct {
		name  string
		value io.WriterTo
	}{{VersionFile, strings.NewReader(CircuitVersion)}, {CircuitFile, circuit}, {ProvingKeyFile, provingKey}, {VerifyingKeyFile, verifyingKey}} {
		if err := writeArtifact(filepath.Join(dir, artifact.name), artifact.value); err != nil {
			return Constraints{}, err
		}
	}
	return Constraints{Constraints: circuit.GetNbConstraints()}, nil
}

// HintsOf returns the eight float32 trigonometric hints of a coordinate.
func HintsOf(c Coordinate) (Hints, error) {
	lat, lng, err := c.radians()
	if err != nil {
		return Hints{}, err
	}
	return Hints{loc2index32.NativeHints(math.Float32bits(lat), math.Float32bits(lng))}, nil
}

// Commit returns the envelope of the secrets, sealing their hints as given.
// It refuses hints of another coordinate, as Prove does, so a server that
// recomputes the envelope at upload refuses them before storing anything.
func Commit(s Secrets) (Envelope, error) {
	lat, lng, salt, hints, err := s.parse()
	if err != nil {
		return Envelope{}, err
	}
	if err := checkHints(lat, lng, hints); err != nil {
		return Envelope{}, err
	}
	value, err := loc2index32.NativeEnvelope(lat, lng, salt, hints)
	if err != nil {
		return Envelope{}, err
	}
	return Envelope{formatScalar(value)}, nil
}

// Prove makes a proof that the hints sealed with the coordinate in s map to
// cell, with the params in dir.
func Prove(dir, cell string, s Secrets) (Proof, error) {
	r, err := RegionOf(cell)
	if err != nil {
		return Proof{}, err
	}
	lat, lng, salt, hints, err := s.parse()
	if err != nil {
		return Proof{}, err
	}
	// Refuse here with a readable message instead of an unsatisfied constraint.
	if err := checkHints(lat, lng, hints); err != nil {
		return Proof{}, err
	}
	if err := checkInside(math.Float32frombits(lat), math.Float32frombits(lng), hints, r); err != nil {
		return Proof{}, err
	}
	envelope, err := loc2index32.NativeEnvelope(lat, lng, salt, hints)
	if err != nil {
		return Proof{}, err
	}

	if err := checkVersion(dir); err != nil {
		return Proof{}, err
	}
	circuit := groth16.NewCS(ecc.BN254)
	if err := readArtifact(filepath.Join(dir, CircuitFile), circuit); err != nil {
		return Proof{}, err
	}
	provingKey := groth16.NewProvingKey(ecc.BN254)
	if err := readArtifact(filepath.Join(dir, ProvingKeyFile), provingKey); err != nil {
		return Proof{}, err
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
		return Proof{}, err
	}
	proof, err := groth16.Prove(circuit, provingKey, witness)
	if err != nil {
		return Proof{}, fmt.Errorf("proving: %w", err)
	}
	var encoded bytes.Buffer
	if _, err := proof.WriteTo(&encoded); err != nil {
		return Proof{}, err
	}
	return Proof{Envelope: formatScalar(envelope), Proof: base64.StdEncoding.EncodeToString(encoded.Bytes())}, nil
}

// Verify returns nil when proofText is a valid proof, under the params in
// dir, that the hints sealed in envelopeText map to cell.
func Verify(dir, cell, envelopeText, proofText string) error {
	r, err := RegionOf(cell)
	if err != nil {
		return err
	}
	envelope, err := parseScalar(envelopeText)
	if err != nil {
		return fmt.Errorf("envelope: %w", err)
	}
	raw, err := base64.StdEncoding.Strict().DecodeString(proofText)
	if err != nil {
		return errors.New("the proof is not base64")
	}
	proof := groth16.NewProof(ecc.BN254)
	reader := bytes.NewReader(raw)
	if _, err := proof.ReadFrom(reader); err != nil || reader.Len() != 0 {
		return errors.New("the proof is not a BN254 Groth16 proof")
	}
	if err := checkVersion(dir); err != nil {
		return err
	}
	verifyingKey := groth16.NewVerifyingKey(ecc.BN254)
	if err := readArtifact(filepath.Join(dir, VerifyingKeyFile), verifyingKey); err != nil {
		return err
	}
	public, err := frontend.NewWitness(&loc2index32.Circuit{
		Envelope: envelope, Resolution: r.Resolution, Face: r.Face, I: r.I, J: r.J, K: r.K,
	}, ecc.BN254.ScalarField(), frontend.PublicOnly())
	if err != nil {
		return err
	}
	if err := groth16.Verify(proof, verifyingKey, public); err != nil {
		return fmt.Errorf("the proof does not verify for this envelope and cell: %w", err)
	}
	return nil
}

// RegionOf returns the tuple the circuit checks for a cell, rejecting cells
// the FP32 circuit cannot map unambiguously.
func RegionOf(text string) (Region, error) {
	cell := h3.Cell(h3.IndexFromString(text))
	if !cell.IsValid() || cell.String() != text {
		return Region{}, fmt.Errorf("%q is not an H3 cell in canonical lowercase form", text)
	}
	if cell.IsPentagon() {
		return Region{}, fmt.Errorf("cell %s is a pentagon, which the circuit does not support", text)
	}
	faces := map[int]bool{}
	for _, face := range cell.IcosahedronFaces() {
		if face >= 0 {
			faces[face] = true
		}
	}
	if len(faces) != 1 {
		return Region{}, fmt.Errorf("cell %s spans %d icosahedron faces; the circuit supports single-face cells", text, len(faces))
	}
	center := cell.LatLng()
	lat, lng := radians(center.Lat), radians(center.Lng)
	if cellAt(lat, lng, cell.Resolution()) != cell {
		return Region{}, fmt.Errorf("the center of cell %s falls outside it at float32 precision", text)
	}
	tuple, err := loc2index32.NativeFaceIJK(lat, lng, cell.Resolution())
	if err != nil {
		return Region{}, err
	}
	if !faces[tuple.Face] {
		return Region{}, fmt.Errorf("the circuit puts cell %s on face %d, not its H3 face", text, tuple.Face)
	}
	return Region{Cell: text, Resolution: cell.Resolution(), Face: tuple.Face, I: tuple.I, J: tuple.J, K: tuple.K}, nil
}

// OutlineOf returns the corners of any valid cell as latitude, longitude
// pairs in degrees, for drawing. It does not check that the circuit supports
// the cell; RegionOf does.
func OutlineOf(text string) (Outline, error) {
	cell := h3.Cell(h3.IndexFromString(text))
	if !cell.IsValid() || cell.String() != text {
		return Outline{}, fmt.Errorf("%q is not an H3 cell in canonical lowercase form", text)
	}
	corners := cell.Boundary()
	boundary := make([][2]float64, len(corners))
	for i, corner := range corners {
		boundary[i] = [2]float64{corner.Lat, corner.Lng}
	}
	return Outline{Cell: text, Resolution: cell.Resolution(), AreaKm2: h3.CellAreaKm2(cell), Boundary: boundary}, nil
}

// CellOf maps a coordinate to the cell at `resolution` that the circuit will
// accept, as the capture step needs before any proof exists.
func CellOf(c Coordinate, resolution int) (Region, error) {
	lat, lng, err := c.radians()
	if err != nil {
		return Region{}, err
	}
	if resolution < 0 || resolution > h3.MaxResolution {
		return Region{}, fmt.Errorf("resolution %d is not between 0 and %d", resolution, h3.MaxResolution)
	}
	r, err := RegionOf(cellAt(lat, lng, resolution).String())
	if err != nil {
		return Region{}, err
	}
	return r, checkInside(lat, lng, loc2index32.NativeHints(math.Float32bits(lat), math.Float32bits(lng)), r)
}

// checkInside runs the circuit's own mapping natively and compares it with
// the region, so a coordinate the circuit would place elsewhere is refused.
func checkInside(lat, lng float32, hints [8]uint32, r Region) error {
	tuple, err := loc2index32.NativeFaceIJKWithHints(hints, r.Resolution)
	if err != nil {
		return err
	}
	if cellAt(lat, lng, r.Resolution).String() != r.Cell ||
		tuple.Face != r.Face || tuple.I != r.I || tuple.J != r.J || tuple.K != r.K {
		return fmt.Errorf("the coordinate is not in cell %s", r.Cell)
	}
	return nil
}

// radians is the coordinate encoding the circuit uses: float32 radians.
func radians(degrees float64) float32 {
	return float32(degrees * math.Pi / 180)
}

func cellAt(lat, lng float32, resolution int) h3.Cell {
	return h3.LatLngToCell(h3.NewLatLng(float64(lat)*180/math.Pi, float64(lng)*180/math.Pi), resolution)
}

// radians returns the coordinate as the circuit takes it, float32 radians.
func (c Coordinate) radians() (lat, lng float32, err error) {
	if !(c.Latitude >= -90 && c.Latitude <= 90 && c.Longitude >= -180 && c.Longitude <= 180) {
		return 0, 0, fmt.Errorf("coordinate (%v, %v) is out of range", c.Latitude, c.Longitude)
	}
	return radians(c.Latitude), radians(c.Longitude), nil
}

// parse returns the coordinate as IEEE-754 bits of float32 radians, the
// salt, and the hints, computed from the coordinate when absent.
func (s Secrets) parse() (lat, lng uint32, salt *big.Int, hints [8]uint32, err error) {
	latRadians, lngRadians, err := s.radians()
	if err != nil {
		return 0, 0, nil, hints, err
	}
	salt, err = parseScalar(s.Salt)
	if err != nil {
		return 0, 0, nil, hints, fmt.Errorf("salt: %w", err)
	}
	lat, lng = math.Float32bits(latRadians), math.Float32bits(lngRadians)
	switch len(s.Hints) {
	case 0:
		if s.Hints != nil {
			return 0, 0, nil, hints, errors.New("hints must hold 8 numbers")
		}
		hints = loc2index32.NativeHints(lat, lng)
	case len(hints):
		copy(hints[:], s.Hints)
	default:
		return 0, 0, nil, hints, fmt.Errorf("hints must hold 8 numbers, not %d", len(s.Hints))
	}
	return lat, lng, salt, hints, nil
}

// checkHints refuses hints that are not the coordinate's own. One float32
// ulp is allowed, for a client whose float64 trigonometry differs from Go's
// in the last bit at a float32 rounding boundary.
func checkHints(lat, lng uint32, hints [8]uint32) error {
	for i, want := range loc2index32.NativeHints(lat, lng) {
		if ulpsApart(hints[i], want) > 1 {
			return fmt.Errorf("hint %d is %#x, but the coordinate gives %#x: the hints belong to another coordinate", i, hints[i], want)
		}
	}
	return nil
}

func ulpsApart(a, b uint32) uint64 {
	ordered := func(bits uint32) int64 {
		if bits&0x80000000 != 0 {
			return -int64(bits & 0x7fffffff)
		}
		return int64(bits)
	}
	difference := ordered(a) - ordered(b)
	if difference < 0 {
		difference = -difference
	}
	return uint64(difference)
}

func checkVersion(dir string) error {
	raw, err := os.ReadFile(filepath.Join(dir, VersionFile))
	if errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("%s has no %s file, so its params are missing or made for an older circuit; run provenance setup again", dir, VersionFile)
	}
	if err != nil {
		return err
	}
	if got := strings.TrimSpace(string(raw)); got != CircuitVersion {
		return fmt.Errorf("%s holds params for circuit %q, not %q; run provenance setup again", dir, got, CircuitVersion)
	}
	return nil
}

func formatScalar(value *big.Int) string {
	return fmt.Sprintf("0x%064x", value)
}

func parseScalar(text string) (*big.Int, error) {
	if len(text) != 66 || text[:2] != "0x" {
		return nil, fmt.Errorf("%q is not 0x followed by 64 hex digits", text)
	}
	raw, err := hex.DecodeString(text[2:])
	if err != nil || formatScalar(new(big.Int).SetBytes(raw)) != text {
		return nil, fmt.Errorf("%q is not lowercase hex", text)
	}
	value := new(big.Int).SetBytes(raw)
	if value.Cmp(fr.Modulus()) >= 0 {
		return nil, fmt.Errorf("%s is not below the BN254 scalar modulus", text)
	}
	return value, nil
}

func writeArtifact(path string, artifact io.WriterTo) error {
	file, err := os.Create(path)
	if err != nil {
		return err
	}
	if _, err := artifact.WriteTo(file); err != nil {
		file.Close()
		return fmt.Errorf("writing %s: %w", path, err)
	}
	return file.Close()
}

func readArtifact(path string, artifact io.ReaderFrom) error {
	file, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("%w; run provenance setup", err)
	}
	defer file.Close()
	if _, err := artifact.ReadFrom(file); err != nil {
		return fmt.Errorf("reading %s: %w", path, err)
	}
	return nil
}
