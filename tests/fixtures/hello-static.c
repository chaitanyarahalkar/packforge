#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static uint32_t checksum(const char *text) {
    uint32_t value = 2166136261u;
    while (*text != '\0') {
        value ^= (uint8_t)*text;
        value *= 16777619u;
        text++;
    }
    return value;
}

int main(int argc, char **argv) {
    const char *environment = getenv("PACKFORGE_SMOKE");
    if (argc != 2 || environment == NULL) {
        fputs("expected one argument and PACKFORGE_SMOKE\n", stderr);
        return 2;
    }

    printf("packforge-smoke argc=%d arg=%s env=%s checksum=%u\n", argc,
           argv[1], environment, checksum(argv[1]) ^ checksum(environment));
    return strcmp(argv[1], "round-trip") == 0 ? 0 : 3;
}
