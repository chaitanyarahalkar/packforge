#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#include "Bcj2.h"
#include "expand.h"

enum { STREAMS = BCJ2_NUM_STREAMS };

struct buffer {
  unsigned char *bytes;
  size_t length;
};

static void fail(void) { abort(); }

static struct buffer read_file(const char *path) {
  FILE *file = fopen(path, "rb");
  if (!file || fseek(file, 0, SEEK_END) != 0) fail();
  const long end = ftell(file);
  if (end < 0 || fseek(file, 0, SEEK_SET) != 0) fail();
  struct buffer result;
  result.length = (size_t)end;
  result.bytes = malloc(result.length ? result.length : 1);
  if (!result.bytes ||
      fread(result.bytes, 1, result.length, file) != result.length)
    fail();
  fclose(file);
  return result;
}

static char *path_with_suffix(const char *prefix, const char *suffix) {
  const size_t length = strlen(prefix) + strlen(suffix) + 1;
  char *path = malloc(length);
  if (!path || snprintf(path, length, "%s%s", prefix, suffix) < 0) fail();
  return path;
}

static struct buffer read_with_suffix(const char *prefix, const char *suffix) {
  char *path = path_with_suffix(prefix, suffix);
  const struct buffer result = read_file(path);
  free(path);
  return result;
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

static void untranspose(unsigned char *bytes, unsigned char *scratch,
                        size_t length) {
  if (length % 4 != 0) fail();
  const size_t values = length / 4;
  memcpy(scratch, bytes, length);
  for (size_t index = 0; index < values; ++index)
    for (size_t byte = 0; byte < 4; ++byte)
      bytes[index * 4 + byte] = scratch[byte * values + index];
}

static void decode_once(const struct buffer compressed[STREAMS],
                        struct buffer decoded[STREAMS],
                        unsigned char *transpose_scratch,
                        unsigned char *output, size_t output_length) {
  for (unsigned stream = BCJ2_STREAM_MAIN; stream <= BCJ2_STREAM_JUMP;
       ++stream) {
    const size_t result = apultra_decompress(
        compressed[stream].bytes, decoded[stream].bytes,
        compressed[stream].length, decoded[stream].length, 0, 0);
    if (result != decoded[stream].length) fail();
  }
  untranspose(decoded[BCJ2_STREAM_JUMP].bytes, transpose_scratch,
              decoded[BCJ2_STREAM_JUMP].length);
  memcpy(decoded[BCJ2_STREAM_RC].bytes, compressed[BCJ2_STREAM_RC].bytes,
         decoded[BCJ2_STREAM_RC].length);

  CBcj2Dec decoder;
  Bcj2Dec_Init(&decoder);
  for (unsigned stream = 0; stream < STREAMS; ++stream) {
    decoder.bufs[stream] = decoded[stream].bytes;
    decoder.lims[stream] = decoded[stream].bytes + decoded[stream].length;
  }
  decoder.dest = output;
  decoder.destLim = output + output_length;
  if (Bcj2Dec_Decode(&decoder) != SZ_OK || decoder.dest != decoder.destLim ||
      !Bcj2Dec_IsMaybeFinished(&decoder))
    fail();
  for (unsigned stream = 0; stream < STREAMS; ++stream)
    if (decoder.bufs[stream] != decoder.lims[stream]) fail();
}

int main(int argc, char **argv) {
  if (argc != 4) return 2;
  const struct buffer original = read_file(argv[1]);
  const char *prefix = argv[2];
  const size_t iterations = (size_t)strtoull(argv[3], NULL, 10);
  if (iterations == 0) fail();
  static const char *const raw_suffixes[STREAMS] = {
      ".main", ".call", ".jump", ".rc"};
  static const char *const compressed_suffixes[STREAMS] = {
      ".main.apu", ".call.apu", ".jump.transpose.apu", ".rc"};
  struct buffer compressed[STREAMS];
  struct buffer decoded[STREAMS];
  for (unsigned stream = 0; stream < STREAMS; ++stream) {
    const struct buffer raw = read_with_suffix(prefix, raw_suffixes[stream]);
    decoded[stream].length = raw.length;
    decoded[stream].bytes = malloc(raw.length ? raw.length : 1);
    if (!decoded[stream].bytes) fail();
    free(raw.bytes);
    compressed[stream] =
        read_with_suffix(prefix, compressed_suffixes[stream]);
  }
  unsigned char *transpose_scratch =
      malloc(decoded[BCJ2_STREAM_JUMP].length
                 ? decoded[BCJ2_STREAM_JUMP].length
                 : 1);
  unsigned char *output = malloc(original.length ? original.length : 1);
  unsigned long long *samples = malloc(iterations * sizeof(*samples));
  if (!transpose_scratch || !output || !samples) fail();

  for (size_t index = 0; index < iterations; ++index) {
    struct timespec start;
    struct timespec end;
    clock_gettime(CLOCK_MONOTONIC_RAW, &start);
    decode_once(compressed, decoded, transpose_scratch, output, original.length);
    clock_gettime(CLOCK_MONOTONIC_RAW, &end);
    if (memcmp(output, original.bytes, original.length) != 0) return 1;
    samples[index] = elapsed_ns(start, end);
  }
  qsort(samples, iterations, sizeof(*samples), compare_u64);
  printf("%llu\n", samples[iterations / 2]);

  free(samples);
  free(output);
  free(transpose_scratch);
  for (unsigned stream = 0; stream < STREAMS; ++stream) {
    free(decoded[stream].bytes);
    free(compressed[stream].bytes);
  }
  free(original.bytes);
  return 0;
}
