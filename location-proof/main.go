// Command location-proof proves that a secret coordinate lies in a public H3
// cell, with the ZKLP FP32 circuit from the C2PA-Thesis fork of zk-Location.
//
// The circuit does not constrain its trigonometric hint outputs back to the
// coordinate, so a modified prover can claim any cell. An accepted proof is
// evidence about an honest prover only.
//
// Secrets are read as JSON from stdin so they stay out of the process list.
// Results are written as JSON to stdout.
package main

import (
	"bytes"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"math"
	"math/big"
	"os"
	"path/filepath"

	"github.com/consensys/gnark-crypto/ecc"
	"github.com/consensys/gnark-crypto/ecc/bn254/fr"
	"github.com/consensys/gnark/backend/groth16"
	"github.com/consensys/gnark/frontend"
	"github.com/consensys/gnark/frontend/cs/r1cs"
	"github.com/consensys/gnark/logger"
	"github.com/tumberger/zk-Location/loc2index32"
	h3 "github.com/uber/h3-go/v4"
)

const usage = `usage:
  location-proof setup  --params DIR
  location-proof region --cell CELL
  location-proof cell   --resolution N           < coordinate.json
  location-proof commit                          < secrets.json
  location-proof prove  --params DIR --cell CELL < secrets.json
  location-proof verify --params DIR --cell CELL --envelope HEX --proof BASE64`

const (
	circuitFile      = "circuit.r1cs"
	provingKeyFile   = "proving.key"
	verifyingKeyFile = "verifying.key"
)

type coordinate struct {
	Latitude  float64 `json:"latitude"`
	Longitude float64 `json:"longitude"`
}

type secrets struct {
	coordinate
	Salt string `json:"salt"`
}

// region is the public tuple the circuit checks the coordinate against.
type region struct {
	Cell       string `json:"cell"`
	Resolution int    `json:"resolution"`
	Face       int    `json:"face"`
	I          int    `json:"i"`
	J          int    `json:"j"`
	K          int    `json:"k"`
}

func main() {
	// gnark logs to stdout, which is reserved for results.
	logger.Disable()
	if err := run(os.Args[1:], os.Stdin, os.Stdout); err != nil {
		fmt.Fprintln(os.Stderr, "location-proof:", err)
		os.Exit(1)
	}
}

func run(args []string, stdin io.Reader, stdout io.Writer) error {
	if len(args) == 0 {
		return errors.New(usage)
	}
	flags := flag.NewFlagSet(args[0], flag.ContinueOnError)
	flags.SetOutput(io.Discard)
	params := flags.String("params", "", "")
	cell := flags.String("cell", "", "")
	resolution := flags.Int("resolution", -1, "")
	envelope := flags.String("envelope", "", "")
	proof := flags.String("proof", "", "")
	if err := flags.Parse(args[1:]); err != nil {
		return fmt.Errorf("%w\n%s", err, usage)
	}

	switch args[0] {
	case "setup":
		if err := required("params", *params); err != nil {
			return err
		}
		return setup(*params, stdout)
	case "region":
		if err := required("cell", *cell); err != nil {
			return err
		}
		r, err := regionOf(*cell)
		if err != nil {
			return err
		}
		return json.NewEncoder(stdout).Encode(r)
	case "cell":
		if *resolution < 0 {
			return fmt.Errorf("--resolution is required\n%s", usage)
		}
		var c coordinate
		if err := readJSON(stdin, &c); err != nil {
			return err
		}
		r, err := cellOf(c, *resolution)
		if err != nil {
			return err
		}
		return json.NewEncoder(stdout).Encode(r)
	case "commit":
		s, err := readSecrets(stdin)
		if err != nil {
			return err
		}
		lat, lng, salt, err := s.parse()
		if err != nil {
			return err
		}
		value, err := loc2index32.NativeEnvelope(lat, lng, salt)
		if err != nil {
			return err
		}
		return json.NewEncoder(stdout).Encode(map[string]string{"envelope": formatScalar(value)})
	case "prove":
		if err := required("params", *params, "cell", *cell); err != nil {
			return err
		}
		s, err := readSecrets(stdin)
		if err != nil {
			return err
		}
		return prove(*params, *cell, s, stdout)
	case "verify":
		if err := required("params", *params, "cell", *cell, "envelope", *envelope, "proof", *proof); err != nil {
			return err
		}
		return verify(*params, *cell, *envelope, *proof)
	}
	return fmt.Errorf("unknown command %q\n%s", args[0], usage)
}

// required takes flag name and value pairs.
func required(pairs ...string) error {
	for i := 0; i < len(pairs); i += 2 {
		if pairs[i+1] == "" {
			return fmt.Errorf("--%s is required\n%s", pairs[i], usage)
		}
	}
	return nil
}

func setup(dir string, stdout io.Writer) error {
	if _, err := os.Stat(filepath.Join(dir, verifyingKeyFile)); err == nil {
		return fmt.Errorf("%s already holds a setup; delete it to run setup again", dir)
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return err
	}
	circuit, err := frontend.Compile(ecc.BN254.ScalarField(), r1cs.NewBuilder, &loc2index32.Circuit{})
	if err != nil {
		return fmt.Errorf("compiling the circuit: %w", err)
	}
	provingKey, verifyingKey, err := groth16.Setup(circuit)
	if err != nil {
		return fmt.Errorf("groth16 setup: %w", err)
	}
	// The verifying key goes last: its presence marks a complete setup.
	for _, artifact := range []struct {
		name  string
		value io.WriterTo
	}{{circuitFile, circuit}, {provingKeyFile, provingKey}, {verifyingKeyFile, verifyingKey}} {
		if err := writeArtifact(filepath.Join(dir, artifact.name), artifact.value); err != nil {
			return err
		}
	}
	return json.NewEncoder(stdout).Encode(map[string]int{"constraints": circuit.GetNbConstraints()})
}

func prove(dir, cellText string, s secrets, stdout io.Writer) error {
	r, err := regionOf(cellText)
	if err != nil {
		return err
	}
	lat, lng, salt, err := s.parse()
	if err != nil {
		return err
	}
	// Refuse here with a readable message instead of an unsatisfied constraint.
	if err := checkInside(math.Float32frombits(lat), math.Float32frombits(lng), r); err != nil {
		return err
	}
	envelope, err := loc2index32.NativeEnvelope(lat, lng, salt)
	if err != nil {
		return err
	}

	circuit := groth16.NewCS(ecc.BN254)
	if err := readArtifact(filepath.Join(dir, circuitFile), circuit); err != nil {
		return err
	}
	provingKey := groth16.NewProvingKey(ecc.BN254)
	if err := readArtifact(filepath.Join(dir, provingKeyFile), provingKey); err != nil {
		return err
	}
	witness, err := frontend.NewWitness(&loc2index32.Circuit{
		Lat: lat, Lng: lng, Salt: salt, Envelope: envelope,
		Resolution: r.Resolution, Face: r.Face, I: r.I, J: r.J, K: r.K,
	}, ecc.BN254.ScalarField())
	if err != nil {
		return err
	}
	proof, err := groth16.Prove(circuit, provingKey, witness)
	if err != nil {
		return fmt.Errorf("proving: %w", err)
	}
	var encoded bytes.Buffer
	if _, err := proof.WriteTo(&encoded); err != nil {
		return err
	}
	return json.NewEncoder(stdout).Encode(map[string]string{
		"envelope": formatScalar(envelope),
		"proof":    base64.StdEncoding.EncodeToString(encoded.Bytes()),
	})
}

func verify(dir, cellText, envelopeText, proofText string) error {
	r, err := regionOf(cellText)
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
	verifyingKey := groth16.NewVerifyingKey(ecc.BN254)
	if err := readArtifact(filepath.Join(dir, verifyingKeyFile), verifyingKey); err != nil {
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

// regionOf returns the tuple the circuit checks for a cell, rejecting cells
// the FP32 circuit cannot map unambiguously.
func regionOf(text string) (region, error) {
	cell := h3.Cell(h3.IndexFromString(text))
	if !cell.IsValid() || cell.String() != text {
		return region{}, fmt.Errorf("%q is not an H3 cell in canonical lowercase form", text)
	}
	if cell.IsPentagon() {
		return region{}, fmt.Errorf("cell %s is a pentagon, which the circuit does not support", text)
	}
	faces := map[int]bool{}
	for _, face := range cell.IcosahedronFaces() {
		if face >= 0 {
			faces[face] = true
		}
	}
	if len(faces) != 1 {
		return region{}, fmt.Errorf("cell %s spans %d icosahedron faces; the circuit supports single-face cells", text, len(faces))
	}
	center := cell.LatLng()
	lat, lng := radians(center.Lat), radians(center.Lng)
	if cellAt(lat, lng, cell.Resolution()) != cell {
		return region{}, fmt.Errorf("the center of cell %s falls outside it at float32 precision", text)
	}
	tuple, err := loc2index32.NativeFaceIJK(lat, lng, cell.Resolution())
	if err != nil {
		return region{}, err
	}
	if !faces[tuple.Face] {
		return region{}, fmt.Errorf("the circuit puts cell %s on face %d, not its H3 face", text, tuple.Face)
	}
	return region{Cell: text, Resolution: cell.Resolution(), Face: tuple.Face, I: tuple.I, J: tuple.J, K: tuple.K}, nil
}

// cellOf maps a coordinate to the cell at `resolution` that the circuit will
// accept, as the capture step needs before any proof exists.
func cellOf(c coordinate, resolution int) (region, error) {
	lat, lng, err := c.radians()
	if err != nil {
		return region{}, err
	}
	if resolution < 0 || resolution > h3.MaxResolution {
		return region{}, fmt.Errorf("resolution %d is not between 0 and %d", resolution, h3.MaxResolution)
	}
	r, err := regionOf(cellAt(lat, lng, resolution).String())
	if err != nil {
		return region{}, err
	}
	return r, checkInside(lat, lng, r)
}

// checkInside runs the circuit's own mapping natively and compares it with
// the region, so a coordinate the circuit would place elsewhere is refused.
func checkInside(lat, lng float32, r region) error {
	tuple, err := loc2index32.NativeFaceIJK(lat, lng, r.Resolution)
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

func readSecrets(stdin io.Reader) (secrets, error) {
	var s secrets
	return s, readJSON(stdin, &s)
}

func readJSON(stdin io.Reader, value any) error {
	decoder := json.NewDecoder(stdin)
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(value); err != nil {
		return fmt.Errorf("reading stdin: %w", err)
	}
	return nil
}

// radians returns the coordinate as the circuit takes it, float32 radians.
func (c coordinate) radians() (lat, lng float32, err error) {
	if !(c.Latitude >= -90 && c.Latitude <= 90 && c.Longitude >= -180 && c.Longitude <= 180) {
		return 0, 0, fmt.Errorf("coordinate (%v, %v) is out of range", c.Latitude, c.Longitude)
	}
	return radians(c.Latitude), radians(c.Longitude), nil
}

// parse returns the coordinate as IEEE-754 bits of float32 radians, and the salt.
func (s secrets) parse() (lat, lng uint32, salt *big.Int, err error) {
	latRadians, lngRadians, err := s.radians()
	if err != nil {
		return 0, 0, nil, err
	}
	salt, err = parseScalar(s.Salt)
	if err != nil {
		return 0, 0, nil, fmt.Errorf("salt: %w", err)
	}
	return math.Float32bits(latRadians), math.Float32bits(lngRadians), salt, nil
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
