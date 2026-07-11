#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>

static std::uint32_t checksum(const char *text) {
    std::uint32_t value = 2166136261u;
    while (*text != '\0') {
        value ^= static_cast<std::uint8_t>(*text);
        value *= 16777619u;
        ++text;
    }
    return value;
}

int main(int argc, char **argv) {
    const char *environment = std::getenv("PACKFORGE_SMOKE");
    if (argc != 2 || environment == nullptr) {
        std::fputs("expected one argument and PACKFORGE_SMOKE\n", stderr);
        return 2;
    }

    std::printf("packforge-smoke argc=%d arg=%s env=%s checksum=%u\n", argc,
                argv[1], environment,
                checksum(argv[1]) ^ checksum(environment));
    return std::strcmp(argv[1], "round-trip") == 0 ? 0 : 3;
}
