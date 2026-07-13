#!/usr/bin/env bash
set -euo pipefail

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/packforge-codec4.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT

PACKFORGE_RUNTIME_V2_DECODER="${PACKFORGE_CODEC4_DECODER:-asm}" \
PACKFORGE_RUNTIME_V2_OUTPUT="$scratch/loader-v2" \
    "$workspace/scripts/build-runtime-v2.sh" --candidate

cc -O2 -Wall -Wextra -Werror -static -no-pie \
    "$workspace/tests/fixtures/hello-static.c" -o "$scratch/original"
cargo run --release --locked -p packforge-core --example m2_codec4_pack -- \
    "$scratch/original" "$scratch/loader-v2" "$scratch/packed"

original_output="$(PACKFORGE_SMOKE=ci "$scratch/original" round-trip)"
packed_output="$(PACKFORGE_SMOKE=ci timeout 10s "$scratch/packed" round-trip)"
test "$packed_output" = "$original_output"

cargo run --release --locked -p packforge-cli -- verify "$scratch/packed" >/dev/null
cargo run --release --locked -p packforge-cli -- unpack \
    "$scratch/packed" --output "$scratch/unpacked" >/dev/null
cmp "$scratch/original" "$scratch/unpacked"
