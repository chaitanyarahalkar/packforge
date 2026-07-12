#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "brotli/decode.h"

static void fail(void) { abort(); }

static unsigned char *read_file(const char *path, size_t *length) {
  FILE *file = fopen(path, "rb");
  if (!file || fseek(file, 0, SEEK_END) != 0) fail();
  const long end = ftell(file);
  if (end < 0 || fseek(file, 0, SEEK_SET) != 0) fail();
  *length = (size_t)end;
  unsigned char *bytes = malloc(*length ? *length : 1);
  if (!bytes || fread(bytes, 1, *length, file) != *length) fail();
  fclose(file);
  return bytes;
}

static unsigned long long elapsed_ns(struct timespec start, struct timespec end) {
  return (unsigned long long)(end.tv_sec - start.tv_sec) * 1000000000ull +
         (unsigned long long)(end.tv_nsec - start.tv_nsec);
}

static int compare_u64(const void *left, const void *right) {
  const unsigned long long a = *(const unsigned long long *)left;
  const unsigned long long b = *(const unsigned long long *)right;
  return (a > b) - (a < b);
}

int main(int argc, char **argv) {
  if (argc != 4) return 2;
  size_t payload_length;
  size_t original_length;
  unsigned char *payload = read_file(argv[1], &payload_length);
  unsigned char *original = read_file(argv[2], &original_length);
  const size_t iterations = (size_t)strtoull(argv[3], NULL, 10);
  unsigned char *output = malloc(original_length ? original_length : 1);
  unsigned long long *samples = malloc(iterations * sizeof(*samples));
  if (!output || !samples || iterations == 0) fail();

  for (size_t index = 0; index < iterations; ++index) {
    size_t output_length = original_length;
    struct timespec start;
    struct timespec end;
    clock_gettime(CLOCK_MONOTONIC_RAW, &start);
    const BrotliDecoderResult result = BrotliDecoderDecompress(
        payload_length, payload, &output_length, output);
    clock_gettime(CLOCK_MONOTONIC_RAW, &end);
    if (result != BROTLI_DECODER_RESULT_SUCCESS ||
        output_length != original_length ||
        memcmp(output, original, original_length) != 0)
      return 1;
    samples[index] = elapsed_ns(start, end);
  }

  qsort(samples, iterations, sizeof(*samples), compare_u64);
  printf("%llu\n", samples[iterations / 2]);
  free(samples);
  free(output);
  free(original);
  free(payload);
  return 0;
}
