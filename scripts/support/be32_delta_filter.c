#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

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

static void write_file(const char *path, const unsigned char *bytes,
                       size_t length) {
  FILE *file = fopen(path, "wb");
  if (!file || fwrite(bytes, 1, length, file) != length || fclose(file) != 0)
    fail();
}

static uint32_t read_be32(const unsigned char *bytes) {
  return (uint32_t)bytes[0] << 24 | (uint32_t)bytes[1] << 16 |
         (uint32_t)bytes[2] << 8 | (uint32_t)bytes[3];
}

static void write_be32(unsigned char *bytes, uint32_t value) {
  bytes[0] = (unsigned char)(value >> 24);
  bytes[1] = (unsigned char)(value >> 16);
  bytes[2] = (unsigned char)(value >> 8);
  bytes[3] = (unsigned char)value;
}

int main(int argc, char **argv) {
  if (argc != 4) return 2;
  const int encoding = argv[1][0] == 'e' && argv[1][1] == '\0';
  const int decoding = argv[1][0] == 'd' && argv[1][1] == '\0';
  const int transpose = argv[1][0] == 't' && argv[1][1] == '\0';
  const int untranspose = argv[1][0] == 'u' && argv[1][1] == '\0';
  if (!encoding && !decoding && !transpose && !untranspose) return 2;
  size_t length;
  unsigned char *bytes = read_file(argv[2], &length);
  if (length % 4 != 0) fail();
  if (encoding || decoding) {
    uint32_t previous = 0;
    for (size_t offset = 0; offset < length; offset += 4) {
      const uint32_t value = read_be32(bytes + offset);
      const uint32_t transformed = encoding ? value - previous : value + previous;
      previous = encoding ? value : transformed;
      write_be32(bytes + offset, transformed);
    }
  } else {
    unsigned char *transformed = malloc(length ? length : 1);
    if (!transformed) fail();
    const size_t values = length / 4;
    for (size_t index = 0; index < values; ++index)
      for (size_t byte = 0; byte < 4; ++byte) {
        const size_t source = transpose ? index * 4 + byte : byte * values + index;
        const size_t destination = transpose ? byte * values + index : index * 4 + byte;
        transformed[destination] = bytes[source];
      }
    free(bytes);
    bytes = transformed;
  }
  write_file(argv[3], bytes, length);
  free(bytes);
  return 0;
}
