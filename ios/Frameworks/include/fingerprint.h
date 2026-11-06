/*
 * C ABI of the lattice fingerprint (crates/fingerprint-ffi), the value the
 * capture device signs and the crop proof checks.
 *
 * Output layout: R, G, then B, FINGERPRINT_HASH_ROWS big-endian 32-byte
 * BLS12-381 scalar field elements each. Hex-encoding each element with a
 * "0x" prefix gives the R, G and B arrays of the fingerprint JSON.
 *
 * Planes are len bytes in row-major order, from 1 to FINGERPRINT_MAX_PIXELS:
 * the pixel at (x, y) is index y * width + x. The hash depends only on the
 * pixel sequence, so the caller signs the width and height with it. The
 * functions keep no global state, are safe to
 * call from any thread, block until done and use every core, so call them
 * off the main thread. Constants are literals so Swift imports them.
 */
#ifndef FINGERPRINT_H
#define FINGERPRINT_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define FINGERPRINT_MAX_PIXELS 524288
#define FINGERPRINT_HASH_ROWS 128
#define FINGERPRINT_ELEMENT_BYTES 32
#define FINGERPRINT_BYTES 12288

/* Status codes. */
#define FINGERPRINT_OK 0
#define FINGERPRINT_ERROR_NULL 1
#define FINGERPRINT_ERROR_LENGTH 2
#define FINGERPRINT_ERROR_ROWS 3
#define FINGERPRINT_ERROR_INTERNAL 4

#ifdef __clang__
#pragma clang assume_nonnull begin
#endif

/* The algorithm identifier for the fingerprint JSON, a static string. */
const char *fingerprint_algorithm(void);

/*
 * Writes FINGERPRINT_BYTES bytes of fingerprint to out. len must be 1 to
 * FINGERPRINT_MAX_PIXELS; out must not overlap the planes.
 */
int32_t fingerprint_rgb(const uint8_t *r, const uint8_t *g, const uint8_t *b,
                        size_t len, uint8_t *out);

/*
 * Writes rows row_start..row_end (end exclusive, at most
 * FINGERPRINT_HASH_ROWS) of one plane's fingerprint, (row_end - row_start) *
 * FINGERPRINT_ELEMENT_BYTES bytes, to out. len must be 1 to
 * FINGERPRINT_MAX_PIXELS; out must not overlap pixels.
 */
int32_t fingerprint_hash_rows(const uint8_t *pixels, size_t len,
                              size_t row_start, size_t row_end, uint8_t *out);

/*
 * Fills three planes of len bytes each with the deterministic test image.
 * With len = FINGERPRINT_MAX_PIXELS (1024x512) the SHA-256 of its
 * fingerprint is
 * cfa3ff823a6ac9e044bf3ba042bb7f7b5fc50ac696ad86e439d03acc273f5083. len must
 * be 1 to FINGERPRINT_MAX_PIXELS; r, g and b must be three buffers that do not
 * overlap.
 */
int32_t fingerprint_test_image(uint8_t *r, uint8_t *g, uint8_t *b, size_t len);

#ifdef __clang__
#pragma clang assume_nonnull end
#endif

#ifdef __cplusplus
}
#endif

#endif /* FINGERPRINT_H */
