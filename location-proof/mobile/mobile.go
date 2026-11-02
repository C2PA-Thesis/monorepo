// Command mobile is package zkloc as a C library for the iOS app. Build it
// with `go build -buildmode=c-archive` (see ios/scripts/build-go.sh); the
// header is ios/Frameworks/include/zkloc.h.
//
// Every export returns a JSON C string, the result or {"error": "..."}, that
// the caller releases with zkloc_free.
package main

/*
#include <stdlib.h>
*/
import "C"

import (
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"unsafe"

	"github.com/C2PA-Thesis/monorepo/location-proof/zkloc"
	"github.com/consensys/gnark/logger"
)

func init() {
	// gnark logs to stdout; the app has no use for it.
	logger.Disable()
}

// main is required by -buildmode=c-archive and never runs.
func main() {}

//export zkloc_hints
func zkloc_hints(lat, lng C.double) *C.char {
	return C.CString(hintsReply(float64(lat), float64(lng)))
}

//export zkloc_envelope
func zkloc_envelope(secretsJSON *C.char) *C.char {
	return C.CString(envelopeReply(goString(secretsJSON)))
}

//export zkloc_prove
func zkloc_prove(paramsDir, cell, secretsJSON *C.char) *C.char {
	return C.CString(proveReply(goString(paramsDir), goString(cell), goString(secretsJSON)))
}

//export zkloc_cell
func zkloc_cell(lat, lng C.double, resolution C.int) *C.char {
	return C.CString(cellReply(float64(lat), float64(lng), int(resolution)))
}

//export zkloc_outline
func zkloc_outline(cell *C.char) *C.char {
	return C.CString(outlineReply(goString(cell)))
}

//export zkloc_free
func zkloc_free(text *C.char) {
	C.free(unsafe.Pointer(text))
}

// goString tells a NULL argument apart from an empty string.
func goString(text *C.char) *string {
	if text == nil {
		return nil
	}
	value := C.GoString(text)
	return &value
}

func hintsReply(lat, lng float64) string {
	return respond(func() (any, error) {
		return zkloc.HintsOf(zkloc.Coordinate{Latitude: lat, Longitude: lng})
	})
}

func envelopeReply(secretsJSON *string) string {
	return respond(func() (any, error) {
		s, err := decodeSecrets(secretsJSON)
		if err != nil {
			return nil, err
		}
		return zkloc.Commit(s)
	})
}

func proveReply(paramsDir, cell, secretsJSON *string) string {
	return respond(func() (any, error) {
		if paramsDir == nil || cell == nil {
			return nil, errors.New("params_dir and cell must not be NULL")
		}
		s, err := decodeSecrets(secretsJSON)
		if err != nil {
			return nil, err
		}
		return zkloc.Prove(*paramsDir, *cell, s)
	})
}

func cellReply(lat, lng float64, resolution int) string {
	return respond(func() (any, error) {
		return zkloc.CellOf(zkloc.Coordinate{Latitude: lat, Longitude: lng}, resolution)
	})
}

// outlineReply draws the cell on the phone, so choosing a size never asks the laptop.
func outlineReply(cell *string) string {
	return respond(func() (any, error) {
		if cell == nil {
			return nil, errors.New("cell must not be NULL")
		}
		return zkloc.OutlineOf(*cell)
	})
}

func decodeSecrets(text *string) (zkloc.Secrets, error) {
	var s zkloc.Secrets
	if text == nil {
		return s, errors.New("secrets_json must not be NULL")
	}
	if err := zkloc.Decode(strings.NewReader(*text), &s); err != nil {
		return s, fmt.Errorf("reading secrets: %w", err)
	}
	return s, nil
}

// respond turns a result into JSON. A panic becomes an error reply, since
// unwinding into the C caller would abort the app. Panics in goroutines that
// gnark starts are not caught here and still abort.
func respond(body func() (any, error)) (reply string) {
	defer func() {
		if recovered := recover(); recovered != nil {
			reply = failure(fmt.Errorf("internal error: %v", recovered))
		}
	}()
	value, err := body()
	if err != nil {
		return failure(err)
	}
	raw, err := json.Marshal(value)
	if err != nil {
		return failure(err)
	}
	return string(raw)
}

func failure(err error) string {
	raw, _ := json.Marshal(map[string]string{"error": err.Error()})
	return string(raw)
}
