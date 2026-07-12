#define _GNU_SOURCE

#include <pthread.h>
#if defined(__linux__)
#include <sched.h>
#endif
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include "Alloc.h"
#include "LzmaDec.h"

enum { CHUNK_COUNT = 4, CHUNK_ENTRY_SIZE = 32, CHUNK_TABLE_SIZE = 128 };

struct chunk {
  size_t decoded_offset;
  size_t decoded_length;
  size_t compressed_offset;
  size_t compressed_length;
  unsigned trailing_bytes;
};

struct decode_task {
  CLzmaDec decoder;
  const unsigned char *input;
  size_t input_length;
  unsigned char *output;
  size_t output_length;
  int result;
};

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

static uint64_t read_u64(const unsigned char *bytes) {
  uint64_t value = 0;
  for (unsigned index = 0; index < 8; ++index)
    value |= (uint64_t)bytes[index] << (index * 8);
  return value;
}

static size_t checked_size(uint64_t value) {
  const size_t result = (size_t)value;
  if ((uint64_t)result != value) fail();
  return result;
}

static void parse_chunks(const unsigned char *payload, size_t payload_length,
                         size_t original_length,
                         struct chunk chunks[CHUNK_COUNT]) {
  if (payload_length <= CHUNK_TABLE_SIZE) fail();
  size_t decoded_offset = 0;
  size_t compressed_offset = CHUNK_TABLE_SIZE;
  for (size_t index = 0; index < CHUNK_COUNT; ++index) {
    const unsigned char *entry = payload + index * CHUNK_ENTRY_SIZE;
    const uint64_t encoded_length = read_u64(entry + 8);
    chunks[index].decoded_offset = checked_size(read_u64(entry));
    chunks[index].decoded_length =
        checked_size(encoded_length & UINT64_C(0x00ffffffffffffff));
    chunks[index].compressed_offset = checked_size(read_u64(entry + 16));
    chunks[index].compressed_length = checked_size(read_u64(entry + 24));
    chunks[index].trailing_bytes = (unsigned)(encoded_length >> 56);
    if (chunks[index].decoded_offset != decoded_offset ||
        chunks[index].compressed_offset != compressed_offset ||
        chunks[index].decoded_length == 0 ||
        chunks[index].compressed_length == 0 ||
        chunks[index].trailing_bytes > 5 ||
        chunks[index].decoded_length > original_length - decoded_offset ||
        chunks[index].compressed_length > payload_length - compressed_offset)
      fail();
    decoded_offset += chunks[index].decoded_length;
    compressed_offset += chunks[index].compressed_length;
  }
  if (decoded_offset != original_length || compressed_offset != payload_length)
    fail();
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

static void task_init(struct decode_task *task, const unsigned char *payload,
                      const struct chunk *chunk, unsigned char *output,
                      const unsigned char properties[LZMA_PROPS_SIZE]) {
  LzmaDec_Construct(&task->decoder);
  if (LzmaDec_AllocateProbs(&task->decoder, properties, LZMA_PROPS_SIZE,
                            &g_Alloc) != SZ_OK)
    fail();
  task->input = payload + chunk->compressed_offset;
  task->input_length = chunk->compressed_length;
  task->output = output + chunk->decoded_offset;
  task->output_length = chunk->decoded_length;
  task->decoder.dic = task->output;
  task->decoder.dicBufSize = task->output_length;
  task->result = 0;
}

static int task_decode(struct decode_task *task) {
  size_t source_length = task->input_length;
  ELzmaStatus status;
  LzmaDec_Init(&task->decoder);
  const SRes result =
      LzmaDec_DecodeToDic(&task->decoder, task->output_length, task->input,
                          &source_length, LZMA_FINISH_ANY, &status);
  task->result = result == SZ_OK && task->decoder.dicPos == task->output_length
                     ? 0
                     : 1;
  return task->result;
}

static void *thread_decode(void *context) {
  (void)task_decode((struct decode_task *)context);
  return NULL;
}

static unsigned long long time_task(struct decode_task *task,
                                    size_t iterations) {
  unsigned long long *samples = malloc(iterations * sizeof(*samples));
  if (!samples) fail();
  for (size_t index = 0; index < iterations; ++index) {
    struct timespec start;
    struct timespec end;
    clock_gettime(CLOCK_MONOTONIC_RAW, &start);
    if (task_decode(task) != 0) fail();
    clock_gettime(CLOCK_MONOTONIC_RAW, &end);
    samples[index] = elapsed_ns(start, end);
  }
  qsort(samples, iterations, sizeof(*samples), compare_u64);
  const unsigned long long median = samples[iterations / 2];
  free(samples);
  return median;
}

static unsigned long long time_parallel(struct decode_task tasks[CHUNK_COUNT],
                                        size_t iterations) {
  unsigned long long *samples = malloc(iterations * sizeof(*samples));
  if (!samples) fail();
  for (size_t iteration = 0; iteration < iterations; ++iteration) {
    pthread_t threads[CHUNK_COUNT - 1];
    struct timespec start;
    struct timespec end;
    clock_gettime(CLOCK_MONOTONIC_RAW, &start);
    for (size_t index = 1; index < CHUNK_COUNT; ++index)
      if (pthread_create(&threads[index - 1], NULL, thread_decode,
                         &tasks[index]) != 0)
        fail();
    if (task_decode(&tasks[0]) != 0) fail();
    for (size_t index = 1; index < CHUNK_COUNT; ++index)
      if (pthread_join(threads[index - 1], NULL) != 0 ||
          tasks[index].result != 0)
        fail();
    clock_gettime(CLOCK_MONOTONIC_RAW, &end);
    samples[iteration] = elapsed_ns(start, end);
  }
  qsort(samples, iterations, sizeof(*samples), compare_u64);
  const unsigned long long median = samples[iterations / 2];
  free(samples);
  return median;
}

static void x86_bcj_decode(unsigned char *bytes, size_t length) {
  static const unsigned char allowed[8] = {1, 1, 1, 0, 1, 0, 0, 0};
  static const unsigned char bit_number[8] = {0, 1, 2, 2, 3, 3, 3, 3};
  if (length <= 4) return;
  const size_t limit = length - 4;
  size_t position = 0;
  size_t previous_position = SIZE_MAX;
  unsigned previous_mask = 0;
  while (position < limit) {
    if ((bytes[position] & 0xfe) != 0xe8) {
      ++position;
      continue;
    }
    previous_position = position - previous_position;
    if (previous_position <= 3) {
      previous_mask = (previous_mask << (previous_position - 1)) & 7;
      if (previous_mask != 0) {
        const unsigned char byte =
            bytes[position + 4 - bit_number[previous_mask]];
        if (!allowed[previous_mask] || byte == 0 || byte == 0xff) {
          previous_position = position;
          previous_mask = (previous_mask << 1) | 1;
          ++position;
          continue;
        }
      }
    } else {
      previous_mask = 0;
    }
    previous_position = position;
    if (bytes[position + 4] == 0 || bytes[position + 4] == 0xff) {
      uint32_t source = (uint32_t)bytes[position + 1] |
                        (uint32_t)bytes[position + 2] << 8 |
                        (uint32_t)bytes[position + 3] << 16 |
                        (uint32_t)bytes[position + 4] << 24;
      uint32_t destination;
      for (;;) {
        destination = source - ((uint32_t)position + 5);
        if (previous_mask == 0) break;
        const unsigned shift = bit_number[previous_mask] * 8;
        const unsigned char byte = (unsigned char)(destination >> (24 - shift));
        if (byte != 0 && byte != 0xff) break;
        source = destination ^ ((UINT32_C(1) << (32 - shift)) - 1);
      }
      destination &= UINT32_C(0x01ffffff);
      destination |= 0u - (destination & UINT32_C(0x01000000));
      bytes[position + 1] = (unsigned char)destination;
      bytes[position + 2] = (unsigned char)(destination >> 8);
      bytes[position + 3] = (unsigned char)(destination >> 16);
      bytes[position + 4] = (unsigned char)(destination >> 24);
      position += 5;
    } else {
      previous_mask = (previous_mask << 1) | 1;
      ++position;
    }
  }
}

static int affinity_cpu_count(long online_cpus) {
#if defined(__linux__)
  (void)online_cpus;
  cpu_set_t affinity;
  CPU_ZERO(&affinity);
  if (sched_getaffinity(0, sizeof(affinity), &affinity) != 0) fail();
  return CPU_COUNT(&affinity);
#else
  return (int)online_cpus;
#endif
}

int main(int argc, char **argv) {
  if (argc != 6) return 2;
  size_t payload_length;
  size_t properties_length;
  size_t original_length;
  unsigned char *payload = read_file(argv[1], &payload_length);
  unsigned char *properties = read_file(argv[2], &properties_length);
  unsigned char *original = read_file(argv[3], &original_length);
  const size_t declared_length = checked_size(strtoull(argv[4], NULL, 10));
  const size_t iterations = checked_size(strtoull(argv[5], NULL, 10));
  if (properties_length != LZMA_PROPS_SIZE || declared_length != original_length ||
      iterations == 0)
    fail();

  struct chunk chunks[CHUNK_COUNT];
  parse_chunks(payload, payload_length, original_length, chunks);
  unsigned char *output = malloc(original_length);
  if (!output) fail();
  struct decode_task tasks[CHUNK_COUNT];
  for (size_t index = 0; index < CHUNK_COUNT; ++index)
    task_init(&tasks[index], payload, &chunks[index], output, properties);

  unsigned long long chunk_ns[CHUNK_COUNT];
  unsigned long long serial_sum_ns = 0;
  unsigned long long four_worker_lower_bound_ns = 0;
  for (size_t index = 0; index < CHUNK_COUNT; ++index) {
    chunk_ns[index] = time_task(&tasks[index], iterations);
    serial_sum_ns += chunk_ns[index];
    if (chunk_ns[index] > four_worker_lower_bound_ns)
      four_worker_lower_bound_ns = chunk_ns[index];
  }
  const unsigned long long two_worker_lower_bound_ns =
      serial_sum_ns / 2 > four_worker_lower_bound_ns
          ? (serial_sum_ns + 1) / 2
          : four_worker_lower_bound_ns;
  const unsigned long long parallel_ns = time_parallel(tasks, iterations);

  x86_bcj_decode(output, original_length);
  if (memcmp(output, original, original_length) != 0) return 1;

  const long online_cpus = sysconf(_SC_NPROCESSORS_ONLN);
  if (online_cpus <= 0) fail();
  const int affinity_cpus = affinity_cpu_count(online_cpus);
  printf("%ld\t%d\t%llu\t%llu\t%llu\t%llu", online_cpus,
         affinity_cpus, serial_sum_ns, two_worker_lower_bound_ns,
         four_worker_lower_bound_ns, parallel_ns);
  for (size_t index = 0; index < CHUNK_COUNT; ++index)
    printf("\t%zu\t%zu\t%llu", chunks[index].decoded_length,
           chunks[index].compressed_length, chunk_ns[index]);
  putchar('\n');

  for (size_t index = 0; index < CHUNK_COUNT; ++index)
    LzmaDec_FreeProbs(&tasks[index].decoder, &g_Alloc);
  free(output);
  free(original);
  free(properties);
  free(payload);
  return 0;
}
