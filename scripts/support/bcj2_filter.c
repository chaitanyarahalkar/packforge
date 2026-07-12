#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "Bcj2.h"

static void fail(void) { abort(); }

static Byte *read_file(const char *path, size_t *length) {
  FILE *file = fopen(path, "rb");
  if (!file || fseek(file, 0, SEEK_END) != 0) fail();
  const long end = ftell(file);
  if (end < 0 || fseek(file, 0, SEEK_SET) != 0) fail();
  *length = (size_t)end;
  Byte *bytes = malloc(*length ? *length : 1);
  if (!bytes || fread(bytes, 1, *length, file) != *length) fail();
  fclose(file);
  return bytes;
}

static void write_file(const char *path, const Byte *bytes, size_t length) {
  FILE *file = fopen(path, "wb");
  if (!file || fwrite(bytes, 1, length, file) != length || fclose(file) != 0)
    fail();
}

static char *stream_path(const char *prefix, unsigned stream) {
  static const char *const suffixes[BCJ2_NUM_STREAMS] = {
      ".main", ".call", ".jump", ".rc"};
  const size_t length = strlen(prefix) + strlen(suffixes[stream]) + 1;
  char *path = malloc(length);
  if (!path) fail();
  if (snprintf(path, length, "%s%s", prefix, suffixes[stream]) < 0) fail();
  return path;
}

static void split(const char *input_path, const char *prefix) {
  size_t input_length;
  Byte *input = read_file(input_path, &input_length);
  Byte *outputs[BCJ2_NUM_STREAMS];
  Byte *starts[BCJ2_NUM_STREAMS];
  CBcj2Enc encoder;
  Bcj2Enc_Init(&encoder);
  Bcj2Enc_SET_FileSize(&encoder, input_length);
  encoder.src = input;
  encoder.srcLim = input + input_length;
  encoder.finishMode = BCJ2_ENC_FINISH_MODE_END_STREAM;
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream) {
    const size_t capacity = input_length + 16;
    outputs[stream] = malloc(capacity);
    if (!outputs[stream]) fail();
    starts[stream] = outputs[stream];
    encoder.bufs[stream] = outputs[stream];
    encoder.lims[stream] = outputs[stream] + capacity;
  }
  Bcj2Enc_Encode(&encoder);
  if (!Bcj2Enc_IsFinished(&encoder) ||
      encoder.state != BCJ2_ENC_STATE_FINISHED || encoder.src != encoder.srcLim)
    fail();
  size_t reconstructed_length = 0;
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream) {
    const size_t length = (size_t)(encoder.bufs[stream] - starts[stream]);
    char *path = stream_path(prefix, stream);
    write_file(path, starts[stream], length);
    free(path);
    if (stream != BCJ2_STREAM_RC) reconstructed_length += length;
  }
  if (reconstructed_length != input_length) fail();
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream)
    free(outputs[stream]);
  free(input);
}

static void join(const char *prefix, size_t output_length,
                 const char *output_path) {
  Byte *inputs[BCJ2_NUM_STREAMS];
  size_t input_lengths[BCJ2_NUM_STREAMS];
  CBcj2Dec decoder;
  Bcj2Dec_Init(&decoder);
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream) {
    char *path = stream_path(prefix, stream);
    inputs[stream] = read_file(path, &input_lengths[stream]);
    free(path);
    decoder.bufs[stream] = inputs[stream];
    decoder.lims[stream] = inputs[stream] + input_lengths[stream];
  }
  Byte *output = malloc(output_length ? output_length : 1);
  if (!output) fail();
  decoder.dest = output;
  decoder.destLim = output + output_length;
  if (Bcj2Dec_Decode(&decoder) != SZ_OK || decoder.dest != decoder.destLim ||
      !Bcj2Dec_IsMaybeFinished(&decoder))
    fail();
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream)
    if (decoder.bufs[stream] != decoder.lims[stream])
      fail();
  write_file(output_path, output, output_length);
  free(output);
  for (unsigned stream = 0; stream < BCJ2_NUM_STREAMS; ++stream)
    free(inputs[stream]);
}

int main(int argc, char **argv) {
  if (argc == 4 && strcmp(argv[1], "split") == 0) {
    split(argv[2], argv[3]);
    return 0;
  }
  if (argc == 5 && strcmp(argv[1], "join") == 0) {
    const unsigned long long value = strtoull(argv[3], NULL, 10);
    if ((size_t)value != value) fail();
    join(argv[2], (size_t)value, argv[4]);
    return 0;
  }
  return 2;
}
