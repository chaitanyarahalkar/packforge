#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "Alloc.h"
#include "LzmaDec.h"

static unsigned char *read_file(const char *path, size_t *length) {
  FILE *file = fopen(path, "rb");
  if (!file || fseek(file, 0, SEEK_END) != 0) abort();
  const long end = ftell(file);
  if (end < 0 || fseek(file, 0, SEEK_SET) != 0) abort();
  *length = (size_t)end;
  unsigned char *bytes = malloc(*length ? *length : 1);
  if (!bytes || fread(bytes, 1, *length, file) != *length) abort();
  fclose(file);
  return bytes;
}

static unsigned long long elapsed_ns(struct timespec start, struct timespec end) {
  return (unsigned long long)(end.tv_sec - start.tv_sec) * 1000000000ull
      + (unsigned long long)(end.tv_nsec - start.tv_nsec);
}

static int compare_u64(const void *left, const void *right) {
  const unsigned long long a = *(const unsigned long long *)left;
  const unsigned long long b = *(const unsigned long long *)right;
  return (a > b) - (a < b);
}

int main(int argc, char **argv) {
  if (argc != 6) return 2;
  size_t payload_length;
  size_t properties_length;
  size_t original_length;
  unsigned char *payload = read_file(argv[1], &payload_length);
  unsigned char *properties = read_file(argv[2], &properties_length);
  unsigned char *original = read_file(argv[3], &original_length);
  const size_t declared_length = (size_t)strtoull(argv[4], NULL, 10);
  const size_t iterations = (size_t)strtoull(argv[5], NULL, 10);
  if (properties_length != LZMA_PROPS_SIZE || declared_length != original_length
      || iterations == 0) abort();

  unsigned char *output = malloc(original_length);
  unsigned long long *samples = malloc(iterations * sizeof(*samples));
  CLzmaDec decoder;
  LzmaDec_Construct(&decoder);
  if (!output || !samples
      || LzmaDec_AllocateProbs(&decoder, properties, LZMA_PROPS_SIZE, &g_Alloc) != SZ_OK) {
    abort();
  }
  decoder.dic = output;
  decoder.dicBufSize = original_length;

  for (size_t index = 0; index < iterations; ++index) {
    size_t source_length = payload_length;
    ELzmaStatus status;
    struct timespec start;
    struct timespec end;
    LzmaDec_Init(&decoder);
    clock_gettime(CLOCK_MONOTONIC_RAW, &start);
    const SRes result = LzmaDec_DecodeToDic(
        &decoder, original_length, payload, &source_length, LZMA_FINISH_ANY, &status);
    clock_gettime(CLOCK_MONOTONIC_RAW, &end);
    if (result != SZ_OK || decoder.dicPos != original_length
        || memcmp(output, original, original_length) != 0) {
      return 1;
    }
    samples[index] = elapsed_ns(start, end);
  }

  qsort(samples, iterations, sizeof(*samples), compare_u64);
  printf("%llu\n", samples[iterations / 2]);
  LzmaDec_FreeProbs(&decoder, &g_Alloc);
  free(samples);
  free(output);
  free(original);
  free(properties);
  free(payload);
  return 0;
}
