#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
    printf 'self-contained executable smoke requires native Linux x86_64\n' >&2
    exit 2
fi

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

c_original="$scratch/hello-c-static"
cpp_original="$scratch/hello-cpp-static"
rust_original="$scratch/hello-rust-static"
go_original="$scratch/hello-go-static"
behavior_original="$scratch/behavior-static"
behavior_packed="$scratch/behavior-packed"
c_packed="$scratch/hello-c-packed"
corrupt_trailer="$scratch/hello-corrupt-trailer"
corrupt_payload="$scratch/hello-corrupt-payload"
target_dir="${CARGO_TARGET_DIR:-$workspace/target}"
if [[ "$target_dir" != /* ]]; then
    target_dir="$workspace/$target_dir"
fi
packer="$target_dir/release/packforge"

"$workspace/scripts/build-runtime-v2.sh" --check

cc -O2 -Wall -Wextra -Werror -static -no-pie \
    "$workspace/tests/fixtures/hello-static.c" -o "$c_original"
c++ -O2 -Wall -Wextra -Werror -static -no-pie \
    "$workspace/tests/fixtures/hello-static.cc" -o "$cpp_original"
rustc --target x86_64-unknown-linux-musl -C opt-level=2 \
    -C relocation-model=static -C link-arg=-no-pie -C strip=symbols \
    "$workspace/tests/fixtures/hello-static.rs" -o "$rust_original"
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -trimpath \
    -ldflags='-s -w -buildid=' -o "$go_original" \
    "$workspace/tests/fixtures/hello-static.go"
cc -O2 -Wall -Wextra -Werror -static -no-pie \
    "$workspace/tests/fixtures/behavior-static.c" -o "$behavior_original"
cargo build --release --locked -p packforge-cli

exercise_fixture() {
    local label="$1"
    local original="$2"
    local packed="$scratch/$label-packed"
    local restored="$scratch/$label-restored"

    "$packer" pack "$original" --output "$packed" --artifact executable \
        --json >/dev/null
    "$packer" inspect "$packed" --json >/dev/null
    "$packer" verify "$packed" --json >/dev/null
    "$packer" unpack "$packed" --output "$restored" --json >/dev/null

    cmp "$original" "$restored"
    local original_output
    local packed_output
    original_output="$(PACKFORGE_SMOKE=ci "$original" round-trip)"
    packed_output="$(PACKFORGE_SMOKE=ci timeout 10s "$packed" round-trip)"
    test "$original_output" = "$packed_output"
    test "$packed_output" = \
        "packforge-smoke argc=2 arg=round-trip env=ci checksum=3954272784"

    local original_size
    local packed_size
    original_size="$(stat -c %s "$original")"
    packed_size="$(stat -c %s "$packed")"
    test "$packed_size" -lt "$original_size"
    printf '%s: %s -> %s bytes\n' "$label" "$original_size" "$packed_size"
}

exercise_fixture hello-c "$c_original"
exercise_fixture hello-cpp "$cpp_original"
exercise_fixture hello-rust "$rust_original"
exercise_fixture hello-go "$go_original"

"$packer" pack "$behavior_original" --output "$behavior_packed" \
    --artifact executable --json >/dev/null
"$packer" verify "$behavior_packed" --json >/dev/null

behavior_directory="$scratch/behavior-run"
mkdir "$behavior_directory"
printf 'descriptor-data' > "$scratch/inherited.txt"
(
    cd "$behavior_directory"
    exec 9<"$scratch/inherited.txt"
    PACKFORGE_SMOKE=ci "$behavior_original" effects \
        >"$scratch/original.stdout" 2>"$scratch/original.stderr"
)
mv "$behavior_directory/effect.txt" "$scratch/original.effect"
(
    cd "$behavior_directory"
    exec 9<"$scratch/inherited.txt"
    PACKFORGE_SMOKE=ci "$behavior_packed" effects \
        >"$scratch/packed.stdout" 2>"$scratch/packed.stderr"
)
mv "$behavior_directory/effect.txt" "$scratch/packed.effect"
cmp "$scratch/original.stdout" "$scratch/packed.stdout"
cmp "$scratch/original.stderr" "$scratch/packed.stderr"
cmp "$scratch/original.effect" "$scratch/packed.effect"

set +e
timeout --preserve-status 5s env PACKFORGE_SMOKE=ci \
    "$behavior_original" signal >/dev/null 2>&1 &
signal_pid="$!"
wait "$signal_pid" 2>/dev/null
original_signal_status="$?"
timeout --preserve-status 5s env PACKFORGE_SMOKE=ci \
    "$behavior_packed" signal >/dev/null 2>&1 &
signal_pid="$!"
wait "$signal_pid" 2>/dev/null
packed_signal_status="$?"
set -e
test "$original_signal_status" -eq "$packed_signal_status"
test "$packed_signal_status" -eq 143
printf 'runtime semantics: cwd, fd, output, effects, status, signal passed\n'

if command -v strace >/dev/null 2>&1; then
    PACKFORGE_SMOKE=ci strace -f -qq -o "$scratch/direct-load.strace" \
        -e trace=execve,execveat,memfd_create,openat,pread64,mmap,mprotect \
        "$c_packed" round-trip >/dev/null
    test "$(grep -c 'execve(' "$scratch/direct-load.strace")" -eq 1
    ! grep -Eq 'execveat\(|memfd_create\(' "$scratch/direct-load.strace"
    printf 'runtime trace: one execve, no memfd_create or execveat passed\n'
fi

file "$c_packed"
readelf -h -l "$c_packed"

c_inspect="$scratch/hello-c.inspect.json"
"$packer" inspect "$c_packed" --json > "$c_inspect"
codec="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["codec"])' \
    "$c_inspect")"
loader_size="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["loader_size"])' \
    "$c_inspect")"
if [[ "$codec" -eq 5 ]]; then
    trailer_failure="packforge: codec-5 runtime failed"
    payload_failure="$trailer_failure"
else
    trailer_failure="packforge: v2 metadata integrity failed"
    payload_failure="packforge: v2 payload integrity failed"
fi

cp "$c_packed" "$corrupt_trailer"
packed_size="$(stat -c %s "$c_packed")"
trailer_field_offset="$((packed_size - 128 + 24))"
printf '\200' | dd of="$corrupt_trailer" bs=1 seek="$trailer_field_offset" \
    conv=notrunc status=none
set +e
failure_output="$("$corrupt_trailer" round-trip 2>&1)"
failure_status="$?"
set -e
test "$failure_status" -eq 127
test "$failure_output" = "$trailer_failure"

cp "$c_packed" "$corrupt_payload"
manifest_size="$("$packer" inspect "$c_packed" --json | \
    python3 -c 'import json,sys; print(json.load(sys.stdin)["manifest_size"])')"
payload_offset="$((loader_size + 192 + manifest_size))"
printf '\200' | dd of="$corrupt_payload" bs=1 seek="$payload_offset" \
    conv=notrunc status=none
set +e
failure_output="$("$corrupt_payload" round-trip 2>&1)"
failure_status="$?"
set -e
test "$failure_status" -eq 127
test "$failure_output" = "$payload_failure"

printf 'native executable differential smoke passed\n'
