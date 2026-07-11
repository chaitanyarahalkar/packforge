#!/usr/bin/env bash
set -euo pipefail

workspace="$(pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

original="$scratch/hello-static"
container="$scratch/hello-static.pfg"
restored="$scratch/hello-restored"
target_dir="${CARGO_TARGET_DIR:-$workspace/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$workspace/$target_dir"
fi
packer="$target_dir/release/packforge"

cc -O2 -Wall -Wextra -Werror -static -no-pie \
  "$workspace/tests/fixtures/hello-static.c" -o "$original"
file "$original"
readelf -h -l "$original"

cargo build --release --locked -p packforge-cli
"$packer" benchmark "$original" --iterations 3 --json
"$packer" pack "$original" --output "$container" --profile auto --json
"$packer" inspect "$container" --json
"$packer" verify "$container" --json
"$packer" unpack "$container" --output "$restored" --json

cmp "$original" "$restored"

original_output="$(PACKFORGE_SMOKE=ci "$original" round-trip)"
restored_output="$(PACKFORGE_SMOKE=ci "$restored" round-trip)"
test "$original_output" = "$restored_output"
test "$restored_output" = "packforge-smoke argc=2 arg=round-trip env=ci checksum=3954272784"

printf '%s\n' "$restored_output"
