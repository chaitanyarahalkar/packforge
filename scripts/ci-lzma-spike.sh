#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  printf 'LZMA feasibility gate requires native Linux x86_64\n' >&2
  exit 2
fi

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

upx_version="5.2.0"
upx_archive="$scratch/upx-$upx_version-amd64_linux.tar.xz"
upx_url="https://github.com/upx/upx/releases/download/v$upx_version/upx-$upx_version-amd64_linux.tar.xz"
upx_sha256="3db5d3294707439db97866feab8d75d800f028f48481a40547411824da4288a1"
loader_limit=23500
fixed_header_bytes=192
trailer_bytes=128

for tool in cc c++ curl go readelf rustc rustup sha256sum tar; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    printf 'missing required tool: %s\n' "$tool" >&2
    exit 2
  fi
done

python3 "$workspace/scripts/benchmark_contract.py" validate-corpus \
  --workspace "$workspace" \
  --corpus "$workspace/benchmarks/corpus-v1.json" >/dev/null

curl --fail --location --silent --show-error "$upx_url" --output "$upx_archive"
printf '%s  %s\n' "$upx_sha256" "$upx_archive" | sha256sum --check --status
tar -xJf "$upx_archive" -C "$scratch"
upx="$scratch/upx-$upx_version-amd64_linux/upx"
test "$("$upx" --version | head -1)" = "upx $upx_version"

rustup toolchain install 1.97.0 --profile minimal \
  --target x86_64-unknown-linux-musl --component llvm-tools-preview >/dev/null
cargo_bin="$(rustup which cargo --toolchain 1.97.0)"
rustc_bin="$(rustup which rustc --toolchain 1.97.0)"
sysroot="$("$rustc_bin" --print sysroot)"
host="$("$rustc_bin" -vV | awk '/^host:/ {print $2}')"
objcopy="$sysroot/lib/rustlib/$host/bin/llvm-objcopy"

runtime_target="$scratch/runtime-target"
(cd "$workspace/runtime/linux-x86_64" && \
  CARGO_TARGET_DIR="$runtime_target" RUSTC="$rustc_bin" \
  "$cargo_bin" build --release --locked --features lzma-size-spike)
runtime_raw="$runtime_target/x86_64-unknown-linux-musl/release/packforge-runtime-linux-x86-64"
runtime="$scratch/loader-lzma-spike"
"$objcopy" --remove-section=.comment "$runtime_raw" "$runtime"
loader_size="$(stat -c %s "$runtime")"
if (( loader_size > loader_limit )); then
  printf 'LZMA runtime is %s bytes; limit is %s\n' "$loader_size" "$loader_limit" >&2
  exit 1
fi
if readelf -lW "$runtime" | grep -q 'INTERP'; then
  printf 'LZMA runtime unexpectedly contains PT_INTERP\n' >&2
  exit 1
fi
if readelf -dW "$runtime" 2>&1 | grep -q 'NEEDED'; then
  printf 'LZMA runtime unexpectedly has dynamic dependencies\n' >&2
  exit 1
fi

spike_target="$scratch/spike-target"
CARGO_TARGET_DIR="$spike_target" "$cargo_bin" build --release --locked \
  --manifest-path "$workspace/runtime/lzma-spike/Cargo.toml"
encoder="$spike_target/release/encode_sdk_rs"

cc -O2 -Wall -Wextra -Werror -static -no-pie \
  "$workspace/tests/fixtures/hello-static.c" -o "$scratch/hello-c"
c++ -O2 -Wall -Wextra -Werror -static -no-pie \
  "$workspace/tests/fixtures/hello-static.cc" -o "$scratch/hello-cpp"
"$rustc_bin" --target x86_64-unknown-linux-musl -C opt-level=2 \
  -C relocation-model=static -C link-arg=-no-pie -C strip=symbols \
  "$workspace/tests/fixtures/hello-static.rs" -o "$scratch/hello-rust"
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -trimpath \
  -ldflags='-s -w -buildid=' -o "$scratch/hello-go" \
  "$workspace/tests/fixtures/hello-static.go"

printf 'fixture\tloader_bytes\tpayload_bytes\tmanifest_bytes\tprojected_bytes\tupx_bytes\tratio_bp\n'
ratios=()
for fixture in hello-c hello-cpp hello-rust hello-go; do
  original="$scratch/$fixture"
  payload="$scratch/$fixture.lzma"
  payload_second="$scratch/$fixture.lzma-second"
  upx_packed="$scratch/$fixture.upx"

  "$encoder" "$original" "$payload"
  "$encoder" "$original" "$payload_second"
  cmp "$payload" "$payload_second"

  cp "$original" "$upx_packed"
  "$upx" --best --quiet "$upx_packed" >/dev/null

  load_segments="$(readelf -lW "$original" | awk '$1 == "LOAD" { count++ } END { print count + 0 }')"
  if (( load_segments < 1 || load_segments > 128 )); then
    printf '%s has unsupported PT_LOAD count %s\n' "$fixture" "$load_segments" >&2
    exit 1
  fi
  manifest_size="$((40 + load_segments * 48))"
  payload_size="$(stat -c %s "$payload")"
  projected_size="$((loader_size + fixed_header_bytes + payload_size + manifest_size + trailer_bytes))"
  upx_size="$(stat -c %s "$upx_packed")"
  ratio_bp="$((projected_size * 10000 / upx_size))"
  if (( ratio_bp > 10500 )); then
    printf '%s projected ratio is %s basis points; limit is 10500\n' \
      "$fixture" "$ratio_bp" >&2
    exit 1
  fi
  ratios+=("$ratio_bp")
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$fixture" "$loader_size" "$payload_size" "$manifest_size" \
    "$projected_size" "$upx_size" "$ratio_bp"
done

mapfile -t sorted_ratios < <(printf '%s\n' "${ratios[@]}" | sort -n)
if (( sorted_ratios[1] + sorted_ratios[2] >= 20000 )); then
  printf 'projected median ratio does not beat UPX: middle ratios %s and %s\n' \
    "${sorted_ratios[1]}" "${sorted_ratios[2]}" >&2
  exit 1
fi
printf 'LZMA feasibility gate passed: loader=%s bytes, median middle-ratio sum=%s (<20000)\n' \
  "$loader_size" "$((sorted_ratios[1] + sorted_ratios[2]))" >&2
