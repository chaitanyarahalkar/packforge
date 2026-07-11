#!/usr/bin/env bash
set -euo pipefail

if (( $# != 2 )); then
  printf 'usage: %s CORPUS_DIRECTORY OUTPUT_MANIFEST\n' "$0" >&2
  exit 2
fi

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
corpus="$1"
output="$2"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
target_dir="${CARGO_TARGET_DIR:-$workspace/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$workspace/$target_dir"
fi
packer="$target_dir/release/packforge"

sha256_file() {
  local path="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    local digest
    digest="$(sha256sum "$path")"
    printf '%s\n' "${digest%% *}"
  else
    local digest
    digest="$(shasum -a 256 "$path")"
    printf '%s\n' "${digest%% *}"
  fi
}

file_size() {
  local path="$1"
  if [[ "$(uname -s)" == "Darwin" ]]; then
    stat -f %z "$path"
  else
    stat -c %s "$path"
  fi
}

cargo build --release --locked -p packforge-cli
printf 'fixture\tprofile\tcodec\tcodec_level\toriginal_sha256\tcontainer_sha256\tcontainer_bytes\n' \
  > "$output"

for fixture in hello-c hello-cpp hello-rust hello-go; do
  original="$corpus/$fixture"
  test -f "$original"
  for profile in fast balanced small auto; do
    container="$scratch/$fixture-$profile.pfg"
    second="$scratch/$fixture-$profile-second.pfg"
    restored="$scratch/$fixture-$profile-restored"
    report="$scratch/$fixture-$profile.json"

    "$packer" pack "$original" --output "$container" --profile "$profile" \
      --allow-larger --json > "$report"
    "$packer" pack "$original" --output "$second" --profile "$profile" \
      --allow-larger --json >/dev/null
    cmp "$container" "$second"
    "$packer" inspect "$container" --json >/dev/null
    "$packer" verify "$container" --json >/dev/null
    "$packer" unpack "$container" --output "$restored" --json >/dev/null
    cmp "$original" "$restored"

    codec_metadata="$(python3 -c \
      'import json,sys; value=json.load(open(sys.argv[1], encoding="utf-8")); print("{}\t{}".format(value["codec"], value["codec_level"]))' \
      "$report")"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
      "$fixture" "$profile" "${codec_metadata%%$'\t'*}" \
      "${codec_metadata#*$'\t'}" "$(sha256_file "$original")" \
      "$(sha256_file "$container")" "$(file_size "$container")" >> "$output"
  done
done

printf 'cross-host container repack passed: %s\n' "$(uname -s)" >&2
