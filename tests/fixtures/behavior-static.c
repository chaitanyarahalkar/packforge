#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int fail(const char *operation) {
    fprintf(stderr, "%s failed: %s\n", operation, strerror(errno));
    return 4;
}

int main(int argc, char **argv) {
    const char *environment = getenv("PACKFORGE_SMOKE");
    if (argc != 2 || environment == NULL) {
        fputs("expected one argument and PACKFORGE_SMOKE\n", stderr);
        return 2;
    }
    if (strcmp(argv[1], "signal") == 0) {
        raise(SIGTERM);
        return 5;
    }

    char working_directory[4096];
    if (getcwd(working_directory, sizeof(working_directory)) == NULL) {
        return fail("getcwd");
    }

    char inherited[64] = {0};
    const ssize_t inherited_length = read(9, inherited, sizeof(inherited) - 1);
    if (inherited_length < 0) {
        return fail("read inherited descriptor");
    }

    const int effect = open("effect.txt", O_WRONLY | O_CREAT | O_TRUNC, 0644);
    if (effect < 0) {
        return fail("open effect");
    }
    char effect_text[256];
    const int effect_length = snprintf(
        effect_text, sizeof(effect_text), "arg=%s env=%s inherited=%s", argv[1],
        environment, inherited);
    if (effect_length < 0 || (size_t)effect_length >= sizeof(effect_text)) {
        close(effect);
        fputs("effect text overflow\n", stderr);
        return 4;
    }
    if (write(effect, effect_text, (size_t)effect_length) != effect_length) {
        close(effect);
        return fail("write effect");
    }
    if (close(effect) != 0) {
        return fail("close effect");
    }

    printf("cwd=%s arg=%s env=%s inherited=%s\n", working_directory, argv[1],
           environment, inherited);
    return 0;
}
