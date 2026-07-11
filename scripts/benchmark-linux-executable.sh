#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
    printf 'executable benchmark requires Linux x86_64\n' >&2
    exit 2
fi

iterations="${1:-21}"
if ! [[ "$iterations" =~ ^[0-9]+$ ]] || (( iterations < 3 || iterations > 101 )); then
    printf 'iterations must be an integer from 3 through 101\n' >&2
    exit 2
fi
if (( iterations % 2 == 0 )); then
    printf 'iterations must be odd so the median is unambiguous\n' >&2
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

upx_version="5.2.0"
upx_archive="$scratch/upx-$upx_version-amd64_linux.tar.xz"
upx_url="https://github.com/upx/upx/releases/download/v$upx_version/upx-$upx_version-amd64_linux.tar.xz"
upx_sha256="3db5d3294707439db97866feab8d75d800f028f48481a40547411824da4288a1"

curl --fail --location --silent --show-error "$upx_url" --output "$upx_archive"
printf '%s  %s\n' "$upx_sha256" "$upx_archive" | sha256sum --check --status
tar -xJf "$upx_archive" -C "$scratch"
upx="$scratch/upx-$upx_version-amd64_linux/upx"
test "$("$upx" --version | head -1)" = "upx $upx_version"

"$workspace/scripts/build-runtime.sh" --check >/dev/null
cargo build --release --locked -p packforge-cli >/dev/null

cc -O2 -Wall -Wextra -Werror -static -no-pie \
    "$workspace/tests/fixtures/hello-static.c" -o "$scratch/hello-c"
c++ -O2 -Wall -Wextra -Werror -static -no-pie \
    "$workspace/tests/fixtures/hello-static.cc" -o "$scratch/hello-cpp"
rustc --target x86_64-unknown-linux-musl -C opt-level=2 \
    -C relocation-model=static -C link-arg=-no-pie -C strip=symbols \
    "$workspace/tests/fixtures/hello-static.rs" -o "$scratch/hello-rust"
CGO_ENABLED=0 GOOS=linux GOARCH=amd64 go build -trimpath \
    -ldflags='-s -w -buildid=' -o "$scratch/hello-go" \
    "$workspace/tests/fixtures/hello-static.go"

measure_time_ns() {
    local executable="$1"
    local samples="$scratch/time-samples"
    : > "$samples"
    PACKFORGE_SMOKE=benchmark "$executable" round-trip >/dev/null
    for ((iteration = 0; iteration < iterations; iteration++)); do
        local start
        local end
        start="$(date +%s%N)"
        PACKFORGE_SMOKE=benchmark "$executable" round-trip >/dev/null
        end="$(date +%s%N)"
        printf '%s\n' "$((end - start))" >> "$samples"
    done
    sort -n "$samples" | sed -n "$(((iterations + 1) / 2))p"
}

measure_rss_kib() {
    local executable="$1"
    local samples="$scratch/rss-samples"
    : > "$samples"
    for ((iteration = 0; iteration < iterations; iteration++)); do
        /usr/bin/time -f '%M' -o "$scratch/rss-one" \
            env PACKFORGE_SMOKE=benchmark "$executable" round-trip >/dev/null
        cat "$scratch/rss-one" >> "$samples"
    done
    sort -n "$samples" | sed -n "$(((iterations + 1) / 2))p"
}

printf 'fixture\tkind\tbytes\tratio_bp\twarm_median_ns\trss_median_kib\n'
for label in hello-c hello-cpp hello-rust hello-go; do
    original="$scratch/$label"
    packforge="$scratch/$label.packforge"
    upx_packed="$scratch/$label.upx"

    "$packer" pack "$original" --output "$packforge" --artifact executable \
        --profile fast --json >/dev/null
    cp "$original" "$upx_packed"
    "$upx" --best --quiet "$upx_packed" >/dev/null

    expected="$(PACKFORGE_SMOKE=benchmark "$original" round-trip)"
    test "$(PACKFORGE_SMOKE=benchmark "$packforge" round-trip)" = "$expected"
    test "$(PACKFORGE_SMOKE=benchmark "$upx_packed" round-trip)" = "$expected"

    original_size="$(stat -c %s "$original")"
    for kind in original packforge upx; do
        case "$kind" in
            original) executable="$original" ;;
            packforge) executable="$packforge" ;;
            upx) executable="$upx_packed" ;;
        esac
        size="$(stat -c %s "$executable")"
        ratio="$((size * 10000 / original_size))"
        time_ns="$(measure_time_ns "$executable")"
        rss_kib="$(measure_rss_kib "$executable")"
        printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$label" "$kind" "$size" "$ratio" "$time_ns" "$rss_kib"
    done
done
