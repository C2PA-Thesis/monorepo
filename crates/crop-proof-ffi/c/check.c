/*
 * Makes and checks a crop proof through the C headers the iOS app uses, at
 * the reference case: the 1024x512 test image, its left 512x512, no turn.
 * Writes the proof to stdout and the time of each step to stderr. Like the
 * app, it links one Rust archive and takes the fingerprint's functions and
 * the allocator from it.
 * Usage: check <parameter directory>
 */
/* clock_gettime is POSIX, which glibc hides under -std=c11. */
#define _POSIX_C_SOURCE 199309L

#include <stdio.h>
#include <string.h>
#include <time.h>

#include "crop_proof.h"
#include "fingerprint.h"

#define WIDTH 1024
#define HEIGHT 512

/* Only Apple targets map large allocations (crates/crop-proof/src/alloc.rs). */
#ifdef __APPLE__
#define MAPPED true
#else
#define MAPPED false
#endif

static int fail(const char *what) {
  fprintf(stderr, "check: %s (%s)\n", what, crop_proof_last_error());
  return 1;
}

static double seconds_since(const struct timespec *start) {
  struct timespec now;
  clock_gettime(CLOCK_MONOTONIC, &now);
  return (double)(now.tv_sec - start->tv_sec) + (double)(now.tv_nsec - start->tv_nsec) / 1e9;
}

int main(int argc, char **argv) {
  if (argc != 2)
    return fail("usage: check <parameter directory>");
  static uint8_t planes[3][WIDTH * HEIGHT];
  static uint8_t proof[CROP_PROOF_CAPACITY];
  const uint8_t *r = planes[0], *g = planes[1], *b = planes[2];
  const crop_proof_rect left = {0, 0, 512, 512};
  const crop_proof_rect right = {512, 0, 512, 512};
  struct timespec start;

  /* This program has one Rust archive, so its allocator must be the one in use. */
  if (crop_proof_large_allocations_are_mapped() != MAPPED)
    return fail(MAPPED ? "the archive's allocator is not in use: a large allocation was not mapped"
                       : "a large allocation was mapped on a system that is not Apple's");

  /* The archive carries the fingerprint's functions: the app links no other Rust library. */
  if (fingerprint_test_image(planes[0], planes[1], planes[2], WIDTH * HEIGHT) != FINGERPRINT_OK)
    return fail("fingerprint_test_image failed");

  crop_proof_prover *prover = (crop_proof_prover *)proof;
  if (crop_proof_prover_load("/nonexistent", &prover) != CROP_PROOF_ERROR_PARAMETERS || prover != NULL ||
      strlen(crop_proof_last_error()) == 0)
    return fail("a missing directory was not refused with a reason");

  clock_gettime(CLOCK_MONOTONIC, &start);
  if (crop_proof_prover_load(argv[1], &prover) != CROP_PROOF_OK)
    return fail("crop_proof_prover_load failed");
  fprintf(stderr, "load the prover: %.2f s\n", seconds_since(&start));

  const crop_proof_rect outside = {513, 0, 512, 512};
  size_t length = 0;
  if (crop_proof_prove(prover, r, g, b, WIDTH, HEIGHT, outside, 0, proof, sizeof proof, &length) !=
      CROP_PROOF_ERROR_ARGUMENT)
    return fail("a rectangle outside the original was not refused");
  if (crop_proof_prove(prover, r, g, b, WIDTH, HEIGHT, left, 4, proof, sizeof proof, &length) !=
      CROP_PROOF_ERROR_ARGUMENT)
    return fail("a turn of four quarters was not refused");

  /* A buffer that is too short gets the length to ask for, and no bytes. */
  size_t needed = 0;
  proof[0] = 0xa5;
  if (crop_proof_prove(prover, r, g, b, WIDTH, HEIGHT, left, 0, proof, 16, &needed) != CROP_PROOF_ERROR_BUFFER ||
      needed <= 16 || needed > sizeof proof || proof[0] != 0xa5)
    return fail("a short buffer did not report the length needed");

  clock_gettime(CLOCK_MONOTONIC, &start);
  if (crop_proof_prove(prover, r, g, b, WIDTH, HEIGHT, left, 0, proof, needed, &length) != CROP_PROOF_OK)
    return fail("crop_proof_prove failed");
  fprintf(stderr, "prove: %.2f s, %zu bytes\n", seconds_since(&start), length);
  if (length != needed)
    return fail("the proof is not as long as the first call said");
  crop_proof_prover_free(prover);

  crop_proof_verifier *verifier = NULL;
  if (crop_proof_verifier_load(argv[1], &verifier) != CROP_PROOF_OK)
    return fail("crop_proof_verifier_load failed");
  clock_gettime(CLOCK_MONOTONIC, &start);
  if (crop_proof_verify(verifier, r, g, b, WIDTH, HEIGHT, left, 0, proof, length) != CROP_PROOF_OK)
    return fail("the proof does not verify");
  fprintf(stderr, "verify: %.2f s\n", seconds_since(&start));

  if (crop_proof_verify(verifier, r, g, b, WIDTH, HEIGHT, right, 0, proof, length) != CROP_PROOF_ERROR_REJECTED)
    return fail("the proof verified for another rectangle");
  if (crop_proof_verify(verifier, r, g, b, WIDTH, HEIGHT, left, 1, proof, length) != CROP_PROOF_ERROR_REJECTED)
    return fail("the proof verified for another turn");
  proof[length / 2] ^= 1;
  if (crop_proof_verify(verifier, r, g, b, WIDTH, HEIGHT, left, 0, proof, length) != CROP_PROOF_ERROR_REJECTED)
    return fail("a damaged proof verified");
  proof[length / 2] ^= 1;
  crop_proof_verifier_free(verifier);
  crop_proof_verifier_free(NULL);

  return fwrite(proof, 1, length, stdout) == length ? 0 : fail("short write");
}
