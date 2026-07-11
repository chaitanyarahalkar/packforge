#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  printf 'container corpus smoke requires native Linux x86_64\n' >&2
  exit 2
fi

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
target_dir="${CARGO_TARGET_DIR:-$workspace/target}"
if [[ "$target_dir" != /* ]]; then
  target_dir="$workspace/$target_dir"
fi
packer="$target_dir/release/packforge"
digest_manifest="${PACKFORGE_CONTAINER_DIGESTS:-$scratch/container-digests.tsv}"
corpus_output="${PACKFORGE_CORPUS_OUTPUT:-}"

python3 "$workspace/scripts/benchmark_contract.py" validate-corpus \
  --workspace "$workspace" \
  --corpus "$workspace/benchmarks/corpus-v1.json" >/dev/null

c_original="$scratch/hello-c"
cpp_original="$scratch/hello-cpp"
rust_original="$scratch/hello-rust"
go_original="$scratch/hello-go"

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

if [[ -n "$corpus_output" ]]; then
  mkdir -p "$corpus_output"
  cp "$c_original" "$cpp_original" "$rust_original" "$go_original" "$corpus_output/"
fi

cargo build --release --locked -p packforge-cli
printf 'fixture\tprofile\tcodec\tcodec_level\toriginal_sha256\tcontainer_sha256\tcontainer_bytes\n' \
  > "$digest_manifest"

exercise_profile() {
  local fixture="$1"
  local original="$2"
  local profile="$3"
  local container="$scratch/$fixture-$profile.pfg"
  local second="$scratch/$fixture-$profile-second.pfg"
  local restored="$scratch/$fixture-$profile-restored"
  local pack_report="$scratch/$fixture-$profile-pack.json"

  "$packer" pack "$original" --output "$container" --profile "$profile" \
    --allow-larger --json > "$pack_report"
  "$packer" pack "$original" --output "$second" --profile "$profile" \
    --allow-larger --json >/dev/null
  cmp "$container" "$second"
  "$packer" inspect "$container" --json >/dev/null
  "$packer" verify "$container" --json >/dev/null
  "$packer" unpack "$container" --output "$restored" --json >/dev/null
  cmp "$original" "$restored"
  test "$(stat -c %a "$original")" = "$(stat -c %a "$restored")"

  local original_output
  local restored_output
  original_output="$(PACKFORGE_SMOKE=ci "$original" round-trip)"
  restored_output="$(PACKFORGE_SMOKE=ci "$restored" round-trip)"
  test "$original_output" = "$restored_output"
  test "$restored_output" = \
    "packforge-smoke argc=2 arg=round-trip env=ci checksum=3954272784"

  local codec_metadata
  codec_metadata="$(python3 -c \
    'import json,sys; value=json.load(open(sys.argv[1], encoding="utf-8")); print("{}\t{}".format(value["codec"], value["codec_level"]))' \
    "$pack_report")"
  local original_digest
  local container_digest
  original_digest="$(sha256sum "$original")"
  original_digest="${original_digest%% *}"
  container_digest="$(sha256sum "$container")"
  container_digest="${container_digest%% *}"
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$fixture" "$profile" "${codec_metadata%%$'\t'*}" \
    "${codec_metadata#*$'\t'}" "$original_digest" "$container_digest" \
    "$(stat -c %s "$container")" >> "$digest_manifest"
}

for fixture in hello-c hello-cpp hello-rust hello-go; do
  case "$fixture" in
    hello-c) original="$c_original" ;;
    hello-cpp) original="$cpp_original" ;;
    hello-rust) original="$rust_original" ;;
    hello-go) original="$go_original" ;;
  esac
  file "$original"
  readelf -h -l "$original" >/dev/null
  for profile in fast balanced small auto; do
    exercise_profile "$fixture" "$original" "$profile"
  done
done

cat "$digest_manifest"
printf 'container corpus matrix passed: 4 fixtures x 4 profiles\n' >&2
