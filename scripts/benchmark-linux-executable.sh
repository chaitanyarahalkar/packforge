#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
    printf 'executable benchmark requires Linux x86_64\n' >&2
    exit 2
fi

warm_iterations="${1:-21}"
cold_iterations="${2:-0}"
if ! [[ "$warm_iterations" =~ ^[0-9]+$ ]] || \
    (( warm_iterations < 3 || warm_iterations > 101 )); then
    printf 'warm iterations must be an integer from 3 through 101\n' >&2
    exit 2
fi
if (( warm_iterations % 2 == 0 )); then
    printf 'warm iterations must be odd so the median is unambiguous\n' >&2
    exit 2
fi
if ! [[ "$cold_iterations" =~ ^[0-9]+$ ]] || \
    (( cold_iterations < 0 || cold_iterations > 31 )); then
    printf 'cold iterations must be zero or an integer from 3 through 31\n' >&2
    exit 2
fi
if (( cold_iterations != 0 && (cold_iterations < 3 || cold_iterations % 2 == 0) )); then
    printf 'cold iterations must be zero or an odd integer from 3 through 31\n' >&2
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
raw_samples="${PACKFORGE_BENCHMARK_RAW:-}"
if [[ -n "$raw_samples" ]]; then
    printf 'fixture\tkind\tmetric\tsample\tvalue\n' > "$raw_samples"
fi

upx_version="5.2.0"
upx_archive="$scratch/upx-$upx_version-amd64_linux.tar.xz"
upx_url="https://github.com/upx/upx/releases/download/v$upx_version/upx-$upx_version-amd64_linux.tar.xz"
upx_sha256="3db5d3294707439db97866feab8d75d800f028f48481a40547411824da4288a1"

python3 "$workspace/scripts/benchmark_contract.py" validate-corpus \
    --workspace "$workspace" \
    --corpus "$workspace/benchmarks/corpus-v1.json" >/dev/null

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

record_sample() {
    local metric="$1"
    local sample="$2"
    local value="$3"
    if [[ -n "$raw_samples" ]]; then
        printf '%s\t%s\t%s\t%s\t%s\n' \
            "$current_label" "$current_kind" "$metric" "$sample" "$value" \
            >> "$raw_samples"
    fi
}

median_sample() {
    local samples="$1"
    local count="$2"
    sort -n "$samples" | sed -n "$(((count + 1) / 2))p"
}

measure_warm_time_ns() {
    local executable="$1"
    local samples="$scratch/time-samples"
    : > "$samples"
    PACKFORGE_SMOKE=benchmark "$executable" round-trip >/dev/null
    for ((iteration = 0; iteration < warm_iterations; iteration++)); do
        local start
        local end
        start="$(date +%s%N)"
        PACKFORGE_SMOKE=benchmark "$executable" round-trip >/dev/null
        end="$(date +%s%N)"
        value="$((end - start))"
        printf '%s\n' "$value" >> "$samples"
        record_sample warm_time_ns "$iteration" "$value"
    done
    median_sample "$samples" "$warm_iterations"
}

measure_rss_kib() {
    local executable="$1"
    local samples="$scratch/rss-samples"
    : > "$samples"
    for ((iteration = 0; iteration < warm_iterations; iteration++)); do
        /usr/bin/time -f '%M' -o "$scratch/rss-one" \
            env PACKFORGE_SMOKE=benchmark "$executable" round-trip >/dev/null
        value="$(<"$scratch/rss-one")"
        printf '%s\n' "$value" >> "$samples"
        record_sample peak_rss_kib "$iteration" "$value"
    done
    median_sample "$samples" "$warm_iterations"
}

drop_linux_page_cache() {
    if [[ "${PACKFORGE_DROP_CACHES:-0}" != "1" ]]; then
        printf 'cold measurements require PACKFORGE_DROP_CACHES=1 on a dedicated runner\n' >&2
        exit 2
    fi
    sync
    if [[ -w /proc/sys/vm/drop_caches ]]; then
        printf '3\n' > /proc/sys/vm/drop_caches
    elif command -v sudo >/dev/null 2>&1 && sudo -n true; then
        sudo -n sh -c 'printf "3\n" > /proc/sys/vm/drop_caches'
    else
        printf 'cannot reset Linux page cache; a dedicated root-capable runner is required\n' >&2
        exit 2
    fi
}

measure_cold_time_ns() {
    local executable="$1"
    if (( cold_iterations == 0 )); then
        printf '0\n'
        return
    fi
    local samples="$scratch/cold-time-samples"
    : > "$samples"
    for ((iteration = 0; iteration < cold_iterations; iteration++)); do
        drop_linux_page_cache
        local start
        local end
        start="$(date +%s%N)"
        PACKFORGE_SMOKE=benchmark "$executable" round-trip >/dev/null
        end="$(date +%s%N)"
        value="$((end - start))"
        printf '%s\n' "$value" >> "$samples"
        record_sample cold_time_ns "$iteration" "$value"
    done
    median_sample "$samples" "$cold_iterations"
}

capture_behavior() {
    local executable="$1"
    local prefix="$2"
    set +e
    PACKFORGE_SMOKE=benchmark "$executable" round-trip \
        > "$prefix.stdout" 2> "$prefix.stderr"
    local status="$?"
    set -e
    printf '%s\n' "$status" > "$prefix.status"
}

printf 'fixture\tkind\tbytes\tratio_bp\tsha256\tbehavior_matches_original\treversible\tdeterministic\twarm_median_ns\tcold_median_ns\trss_median_kib\n'
for label in hello-c hello-cpp hello-rust hello-go; do
    original="$scratch/$label"
    packforge="$scratch/$label.packforge"
    packforge_second="$scratch/$label.packforge-second"
    restored="$scratch/$label.restored"
    upx_packed="$scratch/$label.upx"
    upx_second="$scratch/$label.upx-second"

    "$packer" pack "$original" --output "$packforge" --artifact executable \
        --profile fast --json >/dev/null
    "$packer" pack "$original" --output "$packforge_second" --artifact executable \
        --profile fast --json >/dev/null
    cmp "$packforge" "$packforge_second"
    "$packer" unpack "$packforge" --output "$restored" --json >/dev/null
    cmp "$original" "$restored"
    cp "$original" "$upx_packed"
    cp "$original" "$upx_second"
    "$upx" --best --quiet "$upx_packed" >/dev/null
    "$upx" --best --quiet "$upx_second" >/dev/null
    cmp "$upx_packed" "$upx_second"

    capture_behavior "$original" "$scratch/original-behavior"
    capture_behavior "$packforge" "$scratch/packforge-behavior"
    capture_behavior "$upx_packed" "$scratch/upx-behavior"
    cmp "$scratch/original-behavior.stdout" "$scratch/packforge-behavior.stdout"
    cmp "$scratch/original-behavior.stderr" "$scratch/packforge-behavior.stderr"
    cmp "$scratch/original-behavior.status" "$scratch/packforge-behavior.status"
    cmp "$scratch/original-behavior.stdout" "$scratch/upx-behavior.stdout"
    cmp "$scratch/original-behavior.stderr" "$scratch/upx-behavior.stderr"
    cmp "$scratch/original-behavior.status" "$scratch/upx-behavior.status"

    original_size="$(stat -c %s "$original")"
    for kind in original packforge upx; do
        case "$kind" in
            original) executable="$original" ;;
            packforge) executable="$packforge" ;;
            upx) executable="$upx_packed" ;;
        esac
        size="$(stat -c %s "$executable")"
        ratio="$((size * 10000 / original_size))"
        digest="$(sha256sum "$executable")"
        digest="${digest%% *}"
        current_label="$label"
        current_kind="$kind"
        time_ns="$(measure_warm_time_ns "$executable")"
        cold_time_ns="$(measure_cold_time_ns "$executable")"
        rss_kib="$(measure_rss_kib "$executable")"
        case "$kind" in
            original)
                reversible=true
                deterministic=true
                ;;
            packforge)
                reversible=true
                deterministic=true
                ;;
            upx)
                reversible=false
                deterministic=true
                ;;
        esac
        printf '%s\t%s\t%s\t%s\t%s\ttrue\t%s\t%s\t%s\t%s\t%s\n' \
            "$label" "$kind" "$size" "$ratio" "$digest" \
            "$reversible" "$deterministic" "$time_ns" "$cold_time_ns" "$rss_kib"
    done
done
