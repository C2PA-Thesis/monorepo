/*
 * C ABI of the location circuit (location-proof/mobile, a Go c-archive):
 * hints, envelope, H3 cell and Groth16 proof for a secret coordinate.
 *
 * Every function returns a NUL-terminated UTF-8 JSON string that the caller
 * releases with zkloc_free: the result, or {"error":"<message>"} on failure.
 * No Go panic crosses the boundary. Inputs are borrowed for the call only.
 *
 * Secrets JSON: {"latitude":<deg>,"longitude":<deg>,"salt":"0x<64 hex>",
 * "hints":[8 uint32]}. Latitude, longitude and salt are required; "hints" is
 * optional (the coordinate's own are used) and is refused when it belongs to
 * another coordinate. Unknown fields and trailing data are refused. Results
 * are the same JSON that the location-proof command prints for hints,
 * commit, prove and cell.
 *
 * The functions keep no global state and are safe to call from any thread.
 * zkloc_prove takes seconds and reads the params from disk on every call,
 * so call it off the main thread.
 *
 * zkloc_prove does not check the params themselves: a damaged proving.key
 * can give a proof that does not verify, or exhaust memory and abort the
 * app. Compare the files with the SHA-256 values from the pairing QR first.
 *
 * On iOS the Go runtime needs CoreFoundation (the ZKLocFFI module map links
 * it) and changes the working directory to the app bundle at load, so pass
 * absolute paths.
 */
#ifndef ZKLOC_H
#define ZKLOC_H

#ifdef __cplusplus
extern "C" {
#endif

/* {"hints":[8 uint32]}: float32 bit patterns of the trig hints. */
char *zkloc_hints(double lat, double lng);

/* {"envelope":"0x<64 hex>"}: MiMC over the coordinate, salt and hints. */
char *zkloc_envelope(const char *secrets_json);

/*
 * {"envelope":"0x<64 hex>","proof":"<base64>"}. params_dir holds version,
 * circuit.r1cs and proving.key from `location-proof setup`; cell is an H3
 * index in lowercase hex.
 */
char *zkloc_prove(const char *params_dir, const char *cell,
                  const char *secrets_json);

/* {"cell","resolution","face","i","j","k"}: the cell the circuit accepts. */
char *zkloc_cell(double lat, double lng, int resolution);

/*
 * {"cell","resolution","area_km2","boundary":[[lat,lng],...]}: the corners
 * of any valid cell, for drawing it.
 */
char *zkloc_outline(const char *cell);

/* Releases a string returned above. NULL is allowed. */
void zkloc_free(char *text);

#ifdef __cplusplus
}
#endif

#endif /* ZKLOC_H */
