// Command location-proof proves that a secret coordinate lies in a public H3
// cell, with the ZKLP FP32 circuit from the C2PA-Thesis fork of zk-Location.
// The logic and its trust limits are in package zkloc.
//
// Secrets are read as JSON from stdin so they stay out of the process list.
// Results are written as JSON to stdout.
package main

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"

	"github.com/C2PA-Thesis/monorepo/location-proof/zkloc"
	"github.com/consensys/gnark/logger"
)

const usage = `usage:
  location-proof setup  --params DIR
  location-proof region --cell CELL
  location-proof cell   --resolution N           < coordinate.json
  location-proof outline --cell CELL
  location-proof hints                           < coordinate.json
  location-proof commit                          < secrets.json
  location-proof prove  --params DIR --cell CELL < secrets.json
  location-proof verify --params DIR --cell CELL --envelope HEX --proof BASE64`

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
		return emit(stdout)(zkloc.Setup(*params))
	case "region":
		if err := required("cell", *cell); err != nil {
			return err
		}
		return emit(stdout)(zkloc.RegionOf(*cell))
	case "outline":
		if err := required("cell", *cell); err != nil {
			return err
		}
		return emit(stdout)(zkloc.OutlineOf(*cell))
	case "cell":
		if *resolution < 0 {
			return fmt.Errorf("--resolution is required\n%s", usage)
		}
		var c zkloc.Coordinate
		if err := readJSON(stdin, &c); err != nil {
			return err
		}
		return emit(stdout)(zkloc.CellOf(c, *resolution))
	case "hints":
		var c zkloc.Coordinate
		if err := readJSON(stdin, &c); err != nil {
			return err
		}
		return emit(stdout)(zkloc.HintsOf(c))
	case "commit":
		var s zkloc.Secrets
		if err := readJSON(stdin, &s); err != nil {
			return err
		}
		return emit(stdout)(zkloc.Commit(s))
	case "prove":
		if err := required("params", *params, "cell", *cell); err != nil {
			return err
		}
		var s zkloc.Secrets
		if err := readJSON(stdin, &s); err != nil {
			return err
		}
		return emit(stdout)(zkloc.Prove(*params, *cell, s))
	case "verify":
		if err := required("params", *params, "cell", *cell, "envelope", *envelope, "proof", *proof); err != nil {
			return err
		}
		return zkloc.Verify(*params, *cell, *envelope, *proof)
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

// emit writes a result as one line of JSON, or returns its error.
func emit(stdout io.Writer) func(any, error) error {
	return func(value any, err error) error {
		if err != nil {
			return err
		}
		return json.NewEncoder(stdout).Encode(value)
	}
}

func readJSON(stdin io.Reader, value any) error {
	if err := zkloc.Decode(stdin, value); err != nil {
		return fmt.Errorf("reading stdin: %w", err)
	}
	return nil
}
