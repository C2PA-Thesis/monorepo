/*
 * C ABI of the crop proof (crates/crop-proof-ffi): a proof that a published
 * image is a rectangle of a fingerprinted original, turned by a multiple of
 * 90 degrees.
 *
 * The library is the app's only Rust static library: it also holds the
 * functions of fingerprint.h.
 *
 * An image is three planes, R, G and B, of width * height bytes each in
 * row-major order, the layout fingerprint.h takes: the pixel at (x, y) is
 * index y * width + x. An original has 1 to 524288 pixels.
 *
 * Every function returns a status. crop_proof_last_error gives the reason of
 * a failure in words. A handle is safe to use from several threads at once.
 * Proving blocks until done and uses every core, so call it off the main
 * thread. Constants are literals so Swift imports them.
 */
#ifndef CROP_PROOF_H
#define CROP_PROOF_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/*
 * A buffer of this many bytes holds a proof of the current protocol, which is
 * about 37 KB.
 */
#define CROP_PROOF_CAPACITY 65536

/* Status codes. */
#define CROP_PROOF_OK 0
#define CROP_PROOF_ERROR_NULL 1
/* An image size, rectangle, turn or path the proof does not take. */
#define CROP_PROOF_ERROR_ARGUMENT 2
/* The parameter directory did not load. */
#define CROP_PROOF_ERROR_PARAMETERS 3
/* The proof is longer than the buffer. */
#define CROP_PROOF_ERROR_BUFFER 4
/* The prover failed. */
#define CROP_PROOF_ERROR_PROVE 5
/* The proof does not verify. */
#define CROP_PROOF_ERROR_REJECTED 6
#define CROP_PROOF_ERROR_INTERNAL 7

#ifdef __clang__
#define CROP_PROOF_NULLABLE _Nullable
#define CROP_PROOF_NONNULL _Nonnull
#pragma clang assume_nonnull begin
#else
#define CROP_PROOF_NULLABLE
#define CROP_PROOF_NONNULL
#endif

/* Parameters loaded from a directory that `provenance setup` wrote. */
typedef struct crop_proof_prover crop_proof_prover;
typedef struct crop_proof_verifier crop_proof_verifier;

/* A rectangle of the original, in pixels from its top-left corner. */
typedef struct crop_proof_rect {
  size_t x;
  size_t y;
  size_t width;
  size_t height;
} crop_proof_rect;

/*
 * Whether this library's allocator is the one the program runs on. It gives
 * each Rust allocation of 1 MiB or more a mapping of its own, so a table the
 * prover frees is unmapped at once (crates/crop-proof/src/alloc.rs). Makes
 * one allocation of 2 MiB, frees it, and says whether it was mapped.
 *
 * On Apple systems false means that another Rust static library is linked
 * and the linker took its allocator. On other systems it is always false.
 */
bool crop_proof_large_allocations_are_mapped(void);

/*
 * The reason of the last failed call on this thread. It is empty if no call
 * failed. The string is valid until the next call into this library on the
 * same thread.
 */
const char *crop_proof_last_error(void);

/*
 * Loads the prover parameters from dir, which holds metadata.json and
 * prover.bin, and writes a handle to out. The parameters stay in memory
 * until crop_proof_prover_free. After a failure out holds NULL.
 */
int32_t crop_proof_prover_load(
    const char *dir, crop_proof_prover *CROP_PROOF_NULLABLE *CROP_PROOF_NONNULL out);

/* Frees a prover that no call uses. NULL is allowed. */
void crop_proof_prover_free(crop_proof_prover *CROP_PROOF_NULLABLE prover);

/*
 * Loads the verifier parameters from dir, which holds metadata.json and
 * verifier.bin, and writes a handle to out. After a failure out holds NULL.
 */
int32_t crop_proof_verifier_load(
    const char *dir, crop_proof_verifier *CROP_PROOF_NULLABLE *CROP_PROOF_NONNULL out);

/* Frees a verifier that no call uses. NULL is allowed. */
void crop_proof_verifier_free(crop_proof_verifier *CROP_PROOF_NULLABLE verifier);

/*
 * Proves that rect of the width x height original, turned turn quarter turns
 * clockwise (0 to 3), is a crop of the image with the original's fingerprint.
 * The published image and the fingerprint are computed from the planes.
 *
 * Writes the proof to out and its length to length. If the proof is longer
 * than capacity, out is left alone, length gets the length needed and the
 * status is CROP_PROOF_ERROR_BUFFER. CROP_PROOF_CAPACITY is enough for the
 * current protocol.
 */
int32_t crop_proof_prove(const crop_proof_prover *prover, const uint8_t *r,
                         const uint8_t *g, const uint8_t *b, size_t width,
                         size_t height, crop_proof_rect rect, uint8_t turn,
                         uint8_t *out, size_t capacity, size_t *length);

/*
 * Checks proof as a reader would, against the published image and the
 * fingerprint that the same original gives. It is the check to run on the
 * device right after proving. The status is CROP_PROOF_OK if the proof
 * verifies and CROP_PROOF_ERROR_REJECTED if it does not.
 */
int32_t crop_proof_verify(const crop_proof_verifier *verifier, const uint8_t *r,
                          const uint8_t *g, const uint8_t *b, size_t width,
                          size_t height, crop_proof_rect rect, uint8_t turn,
                          const uint8_t *proof, size_t proof_length);

#ifdef __clang__
#pragma clang assume_nonnull end
#endif

#ifdef __cplusplus
}
#endif

#endif /* CROP_PROOF_H */
