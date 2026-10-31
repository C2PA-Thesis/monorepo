/*
 * Calls the fingerprint through the C header the iOS app uses. Writes the
 * test image's fingerprint to stdout, for the caller to hash, and the time
 * of each run to stderr. Usage: check [runs]
 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "fingerprint.h"

static int fail(const char *what) {
  fprintf(stderr, "check: %s\n", what);
  return 1;
}

int main(int argc, char **argv) {
  int runs = argc > 1 ? atoi(argv[1]) : 1;
  static uint8_t planes[3][FINGERPRINT_PIXELS], ours[3][FINGERPRINT_PIXELS];
  static uint8_t out[FINGERPRINT_BYTES];

  /* The same formula as the Rust and TypeScript test image, written out
     again so the check does not only trust fingerprint_test_image. */
  for (size_t channel = 0; channel < 3; channel++)
    for (size_t i = 0; i < FINGERPRINT_PIXELS; i++)
      ours[channel][i] = (uint8_t)((i * 7 + channel * 13 + (i >> 9)) & 0xff);
  if (fingerprint_test_image(planes[0], planes[1], planes[2], FINGERPRINT_PIXELS) != FINGERPRINT_OK)
    return fail("fingerprint_test_image failed");
  if (memcmp(planes, ours, sizeof planes) != 0)
    return fail("fingerprint_test_image differs from the formula");

  if (fingerprint_rgb(planes[0], planes[1], planes[2], FINGERPRINT_PIXELS - 1, out) !=
      FINGERPRINT_ERROR_LENGTH)
    return fail("a short plane was not rejected");
  if (fingerprint_hash_rows(planes[0], FINGERPRINT_PIXELS, 0, FINGERPRINT_HASH_ROWS + 1, out) !=
      FINGERPRINT_ERROR_ROWS)
    return fail("rows past the end were not rejected");

  for (int run = 0; run < runs; run++) {
    struct timespec start, end;
    clock_gettime(CLOCK_MONOTONIC, &start);
    int status = fingerprint_rgb(planes[0], planes[1], planes[2], FINGERPRINT_PIXELS, out);
    clock_gettime(CLOCK_MONOTONIC, &end);
    if (status != FINGERPRINT_OK)
      return fail("fingerprint_rgb failed");
    double ms = (end.tv_sec - start.tv_sec) * 1e3 + (end.tv_nsec - start.tv_nsec) / 1e6;
    fprintf(stderr, "fingerprint_rgb run %d: %.1f ms\n", run + 1, ms);
  }

  uint8_t rows[2 * FINGERPRINT_ELEMENT_BYTES];
  if (fingerprint_hash_rows(planes[1], FINGERPRINT_PIXELS, 5, 7, rows) != FINGERPRINT_OK)
    return fail("fingerprint_hash_rows failed");
  if (memcmp(rows, out + (FINGERPRINT_HASH_ROWS + 5) * FINGERPRINT_ELEMENT_BYTES, sizeof rows) != 0)
    return fail("fingerprint_hash_rows differs from fingerprint_rgb");

  fprintf(stderr, "algorithm: %s\n", fingerprint_algorithm());
  return fwrite(out, 1, sizeof out, stdout) == sizeof out ? 0 : fail("short write");
}
