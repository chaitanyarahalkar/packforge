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
runtime_traces="${PACKFORGE_RUNTIME_TRACES:-}"
phase_iterations="${PACKFORGE_PHASE_ITERATIONS:-0}"
codec_spike_output="${PACKFORGE_CODEC_SPIKE_OUTPUT:-}"
asm_oracle_output="${PACKFORGE_ASM_ORACLE_OUTPUT:-}"
asm_object_output="${PACKFORGE_ASM_OBJECT_OUTPUT:-}"
brotli_output="${PACKFORGE_BROTLI_OUTPUT:-}"
apultra_bcj2_output="${PACKFORGE_APULTRA_BCJ2_OUTPUT:-}"
codec5_partition_output="${PACKFORGE_CODEC5_PARTITION_OUTPUT:-}"
pfg_lz_output="${PACKFORGE_PFG_LZ_OUTPUT:-}"
pfg_hlz_output="${PACKFORGE_PFG_HLZ_OUTPUT:-}"
force_codec4="${PACKFORGE_BENCHMARK_CODEC4:-0}"
runtime_candidate="${PACKFORGE_BENCHMARK_RUNTIME_CANDIDATE:-0}"
if ! [[ "$phase_iterations" =~ ^[0-9]+$ ]] || \
    (( phase_iterations < 0 || phase_iterations > 21 )); then
    printf 'PACKFORGE_PHASE_ITERATIONS must be from 0 through 21\n' >&2
    exit 2
fi
if [[ "$force_codec4" != "0" && "$force_codec4" != "1" ]]; then
    printf 'PACKFORGE_BENCHMARK_CODEC4 must be 0 or 1\n' >&2
    exit 2
fi
if [[ "$runtime_candidate" != "0" && "$runtime_candidate" != "1" ]]; then
    printf 'PACKFORGE_BENCHMARK_RUNTIME_CANDIDATE must be 0 or 1\n' >&2
    exit 2
fi
if [[ -n "$raw_samples" ]]; then
    printf 'fixture\tkind\tmetric\tsample\tvalue\n' > "$raw_samples"
fi
if [[ -n "$runtime_traces" ]]; then
    if ! command -v strace >/dev/null 2>&1; then
        printf 'PACKFORGE_RUNTIME_TRACES requires strace\n' >&2
        exit 2
    fi
    mkdir -p "$runtime_traces"
fi
if [[ -n "$codec_spike_output" ]]; then
    printf 'fixture\tstreams\tpayload_bytes\tprojected_bytes\tupx_ceiling_bytes\tpasses_size\n' \
        > "$codec_spike_output"
fi
if [[ -n "$asm_oracle_output" ]]; then
    printf 'fixture\tonline_cpus\taffinity_cpus\tserial_sum_ns\ttwo_worker_lower_bound_ns\tfour_worker_lower_bound_ns\tpthread_parallel_median_ns\tchunk0_decoded_bytes\tchunk0_compressed_bytes\tchunk0_median_ns\tchunk1_decoded_bytes\tchunk1_compressed_bytes\tchunk1_median_ns\tchunk2_decoded_bytes\tchunk2_compressed_bytes\tchunk2_median_ns\tchunk3_decoded_bytes\tchunk3_compressed_bytes\tchunk3_median_ns\tasm_object_bytes\tasm_text_bytes\n' > "$asm_oracle_output"
fi
if [[ -n "$brotli_output" ]]; then
    printf 'fixture\tpayload_bytes\tmedian_decode_ns\tupx_ceiling_bytes\tmanifest_bytes\tmaximum_complete_loader_bytes\tdecoder_file_bytes\tdecoder_text_bytes\tdecoder_rodata_bytes\tdecoder_linked_lower_bound_bytes\tdecoder_lower_bound_fits\n' > "$brotli_output"
fi
if [[ -n "$apultra_bcj2_output" ]]; then
    printf 'fixture\truntime_image_bytes\trecovery_tail_bytes\tpayload_bytes\tmedian_decode_ns\trust_main_decode_ns\trust_call_decode_ns\trust_jump_decode_ns\trust_bcj2_decode_ns\trust_original_hash_ns\tupx_bytes\tupx_ceiling_bytes\tmaximum_loader_for_upx_bytes\tmaximum_loader_for_105_percent_bytes\tbare_loader_bytes\toracle_text_bytes\toracle_rodata_bytes\tlinked_loader_lower_bound_bytes\tprojected_bytes\n' > "$apultra_bcj2_output"
fi
if [[ -n "$codec5_partition_output" ]]; then
    printf 'fixture\tstreams\tmain_decoded_bytes\twhole_main_compressed_bytes\tsplit_main_compressed_bytes\tmain_delta_bytes\tfixed_table_delta_bytes\tmaximum_loader_for_upx_bytes\tremaining_loader_bytes\tformat_budget_pass\n' \
        > "$codec5_partition_output"
fi
if [[ -n "$pfg_lz_output" ]]; then
    printf 'fixture\toriginal_bytes\tpfglz_payload_bytes\tcodec5_payload_bytes\tpfglz_over_codec5_bp\n' \
        > "$pfg_lz_output"
fi
if [[ -n "$pfg_hlz_output" ]]; then
    printf 'fixture\toriginal_bytes\tpfghlz_payload_bytes\tcodec5_payload_bytes\tpfghlz_over_codec5_bp\n' \
        > "$pfg_hlz_output"
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

if [[ "$runtime_candidate" == "1" ]]; then
    PACKFORGE_RUNTIME_V2_DECODER=parallel \
    PACKFORGE_RUNTIME_V2_HASH=compact-four-optz \
    PACKFORGE_RUNTIME_V2_OUTPUT="$workspace/runtime/artifacts/linux-x86_64/loader-v2" \
        "$workspace/scripts/build-runtime-v2.sh" --candidate >/dev/null
    PACKFORGE_RUNTIME_V2_DECODER=apultra-bcj2 \
    PACKFORGE_RUNTIME_V2_DECODER_OPT_LEVEL=2 \
    PACKFORGE_RUNTIME_V2_HASH=compact-optz \
    PACKFORGE_RUNTIME_V2_OUTPUT="$workspace/runtime/artifacts/linux-x86_64/loader-v2-codec5" \
        "$workspace/scripts/build-runtime-v2.sh" --candidate >/dev/null
else
    "$workspace/scripts/build-runtime-v2.sh" --check >/dev/null
fi
cargo build --release --locked -p packforge-cli >/dev/null
if [[ "$force_codec4" == "1" ]]; then
    cargo build --release --locked -p packforge-core --example m2_codec4_pack >/dev/null
    codec4_packer="$target_dir/release/examples/m2_codec4_pack"
fi
if [[ -n "$codec_spike_output" ]]; then
    cargo build --release --locked -p packforge-core --example m2_codec_spike >/dev/null
    codec_spike="$target_dir/release/examples/m2_codec_spike"
fi
if [[ -n "$codec5_partition_output" ]]; then
    cargo build --release --locked -p packforge-core \
        --example m2_codec5_partition_spike >/dev/null
    codec5_partition_spike="$target_dir/release/examples/m2_codec5_partition_spike"
fi
if [[ -n "$pfg_lz_output" ]]; then
    cargo build --release --locked -p packforge-core --example m2_pfg_lz_probe >/dev/null
    pfg_lz_probe="$target_dir/release/examples/m2_pfg_lz_probe"
fi
if [[ -n "$pfg_hlz_output" ]]; then
    cargo build --release --locked -p packforge-core --example m2_pfg_hlz_probe >/dev/null
    pfg_hlz_probe="$target_dir/release/examples/m2_pfg_hlz_probe"
fi
if [[ -n "$asm_oracle_output" ]]; then
    seven_zip_commit="f9d78aff31a5f2521ae7ddbdc97c4a8855808959"
    asmc_commit="4b669147521b277b9e050922e7c97cb8aa608f45"
    git clone --quiet --no-checkout https://github.com/ip7z/7zip.git "$scratch/7zip"
    git -C "$scratch/7zip" checkout --quiet "$seven_zip_commit"
    test "$(git -C "$scratch/7zip" rev-parse HEAD)" = "$seven_zip_commit"
    git clone --quiet --no-checkout https://github.com/nidud/asmc.git "$scratch/asmc"
    git -C "$scratch/asmc" checkout --quiet "$asmc_commit"
    test "$(git -C "$scratch/asmc" rev-parse HEAD)" = "$asmc_commit"
    chmod +x "$scratch/asmc/bin/asmc64"
    (
        cd "$scratch/7zip/Asm/x86"
        "$scratch/asmc/bin/asmc64" -elf64 -DABI_LINUX \
            -Fo"$scratch/" LzmaDecOpt.asm
    ) >/dev/null
    cc -O3 -DNDEBUG -DZ7_LZMA_DEC_OPT -Wall -Wextra -Werror \
        -I"$scratch/7zip/C" \
        "$workspace/scripts/support/lzma_asm_oracle.c" \
        "$scratch/7zip/C/LzmaDec.c" "$scratch/7zip/C/Alloc.c" \
        "$scratch/LzmaDecOpt.o" -o "$scratch/lzma-asm-oracle"
    cc -O3 -DNDEBUG -DZ7_LZMA_DEC_OPT -Wall -Wextra -Werror -pthread \
        -I"$scratch/7zip/C" \
        "$workspace/scripts/support/lzma_codec4_asm_oracle.c" \
        "$scratch/7zip/C/LzmaDec.c" "$scratch/7zip/C/Alloc.c" \
        "$scratch/LzmaDecOpt.o" -o "$scratch/lzma-codec4-asm-oracle"
    asm_object_bytes="$(stat -c %s "$scratch/LzmaDecOpt.o")"
    asm_text_bytes="$(size -A "$scratch/LzmaDecOpt.o" | awk '$1 ~ /^\.text/ { total += $2 } END { print total + 0 }')"
    if [[ -n "$asm_object_output" ]]; then
        install -m 0644 "$scratch/LzmaDecOpt.o" "$asm_object_output"
    fi
fi
if [[ -n "$brotli_output" ]]; then
    brotli_commit="028fb5a23661f123017c060daa546b55cf4bde29"
    git clone --quiet --no-checkout https://github.com/google/brotli.git "$scratch/brotli"
    git -C "$scratch/brotli" checkout --quiet "$brotli_commit"
    test "$(git -C "$scratch/brotli" rev-parse HEAD)" = "$brotli_commit"
    cmake -S "$scratch/brotli" -B "$scratch/brotli-build" \
        -DCMAKE_BUILD_TYPE=MinSizeRel \
        -DCMAKE_C_FLAGS_MINSIZEREL='-Os -DNDEBUG -ffunction-sections -fdata-sections' \
        -DBROTLI_BUILD_FOR_PACKAGE=ON -DBROTLI_DISABLE_TESTS=ON >/dev/null
    cmake --build "$scratch/brotli-build" \
        --target brotli brotlidec-static brotlicommon-static -j 4 >/dev/null
    cc -Os -DNDEBUG -Wall -Wextra -Werror -ffunction-sections -fdata-sections \
        -I"$scratch/brotli/c/include" \
        "$workspace/scripts/support/brotli_decoder_oracle.c" \
        "$scratch/brotli-build/libbrotlidec-static.a" \
        "$scratch/brotli-build/libbrotlicommon-static.a" \
        -Wl,--gc-sections -lm -o "$scratch/brotli-decoder-oracle"
    brotli_decoder_file_bytes="$(stat -c %s "$scratch/brotli-decoder-oracle")"
    brotli_text_bytes="$(size -A "$scratch/brotli-decoder-oracle" | \
        awk '$1 == ".text" { print $2 + 0 }')"
    brotli_rodata_bytes="$(size -A "$scratch/brotli-decoder-oracle" | \
        awk '$1 == ".rodata" { print $2 + 0 }')"
    brotli_linked_lower_bound="$((brotli_text_bytes + brotli_rodata_bytes))"
fi
if [[ -n "$apultra_bcj2_output" ]]; then
    if [[ ! -d "$scratch/7zip" ]]; then
        seven_zip_commit="f9d78aff31a5f2521ae7ddbdc97c4a8855808959"
        git clone --quiet --no-checkout https://github.com/ip7z/7zip.git "$scratch/7zip"
        git -C "$scratch/7zip" checkout --quiet "$seven_zip_commit"
        test "$(git -C "$scratch/7zip" rev-parse HEAD)" = "$seven_zip_commit"
    fi
    apultra_commit="8f340057d7402c10da3d9c76c599f9ab83b8a22d"
    git clone --quiet --no-checkout https://github.com/emmanuel-marty/apultra.git \
        "$scratch/apultra"
    git -C "$scratch/apultra" checkout --quiet "$apultra_commit"
    test "$(git -C "$scratch/apultra" rev-parse HEAD)" = "$apultra_commit"
    make -C "$scratch/apultra" -s all
    cc -O2 -Wall -Wextra -Werror -I"$scratch/7zip/C" \
        "$workspace/scripts/support/bcj2_filter.c" \
        "$scratch/7zip/C/Bcj2Enc.c" "$scratch/7zip/C/Bcj2.c" \
        -o "$scratch/bcj2-filter"
    cc -O2 -Wall -Wextra -Werror \
        "$workspace/scripts/support/be32_delta_filter.c" \
        -o "$scratch/be32-filter"
    cc -Os -DNDEBUG -Wall -Wextra -Werror -Wno-unused-parameter \
        -ffunction-sections -fdata-sections \
        -I"$scratch/7zip/C" -I"$scratch/apultra/src" \
        -I"$scratch/apultra/src/libdivsufsort/include" \
        "$workspace/scripts/support/apultra_bcj2_oracle.c" \
        "$scratch/apultra/src/expand.c" "$scratch/7zip/C/Bcj2.c" \
        -Wl,--gc-sections -o "$scratch/apultra-bcj2-oracle"
    apultra_oracle_text_bytes="$(size -A "$scratch/apultra-bcj2-oracle" | \
        awk '$1 == ".text" { print $2 + 0 }')"
    apultra_oracle_rodata_bytes="$(size -A "$scratch/apultra-bcj2-oracle" | \
        awk '$1 == ".rodata" { print $2 + 0 }')"
    cargo build --release --locked -p packforge-core --example m2_lzma_encode >/dev/null
    apultra_lzma_encoder="$target_dir/release/examples/m2_lzma_encode"
    PACKFORGE_RUNTIME_V2_DECODER=none \
        PACKFORGE_RUNTIME_V2_DECODER_OPT_LEVEL= \
        PACKFORGE_RUNTIME_V2_OUTPUT="$scratch/loader-v2-bare" \
        "$workspace/scripts/build-runtime-v2.sh" --candidate >/dev/null
    apultra_bare_loader_bytes="$(stat -c %s "$scratch/loader-v2-bare")"
    apultra_linked_loader_lower_bound="$((apultra_bare_loader_bytes + apultra_oracle_text_bytes + apultra_oracle_rodata_bytes))"
fi

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

    if [[ "$force_codec4" == "1" ]]; then
        "$codec4_packer" "$original" \
            "$workspace/runtime/artifacts/linux-x86_64/loader-v2" "$packforge"
        "$codec4_packer" "$original" \
            "$workspace/runtime/artifacts/linux-x86_64/loader-v2" "$packforge_second"
    else
        "$packer" pack "$original" --output "$packforge" --artifact executable \
            --profile balanced --json >/dev/null
        "$packer" pack "$original" --output "$packforge_second" --artifact executable \
            --profile balanced --json >/dev/null
    fi
    cmp "$packforge" "$packforge_second"
    "$packer" unpack "$packforge" --output "$restored" --json >/dev/null
    cmp "$original" "$restored"
    cp "$original" "$upx_packed"
    cp "$original" "$upx_second"
    "$upx" --best --quiet "$upx_packed" >/dev/null
    "$upx" --best --quiet "$upx_second" >/dev/null
    cmp "$upx_packed" "$upx_second"

    if [[ -n "$pfg_lz_output" ]]; then
        "$pfg_lz_probe" "$label" "$original" >> "$pfg_lz_output"
    fi
    if [[ -n "$pfg_hlz_output" ]]; then
        "$pfg_hlz_probe" "$label" "$original" >> "$pfg_hlz_output"
    fi

    if [[ -n "$apultra_bcj2_output" ]]; then
        apultra_inspect="$scratch/$label.apultra-inspect.json"
        "$packer" inspect "$packforge" --json > "$apultra_inspect"
        apultra_runtime_length="$(python3 -c \
            'import json,sys; value=json.load(open(sys.argv[1])); print(max(item["file_offset"] + item["file_size"] for item in value["manifest"]["segments"]))' \
            "$apultra_inspect")"
        apultra_original_length="$(stat -c %s "$original")"
        apultra_tail_length="$((apultra_original_length - apultra_runtime_length))"
        apultra_runtime="$scratch/$label.apultra-runtime"
        apultra_tail="$scratch/$label.apultra-tail"
        dd if="$original" of="$apultra_runtime" bs=1 count="$apultra_runtime_length" status=none
        dd if="$original" of="$apultra_tail" bs=1 skip="$apultra_runtime_length" status=none
        apultra_prefix="$scratch/$label.apultra-bcj2"
        "$scratch/bcj2-filter" split "$apultra_runtime" "$apultra_prefix"
        "$scratch/be32-filter" t "$apultra_prefix.jump" "$apultra_prefix.jump.transpose"
        "$scratch/apultra/apultra" "$apultra_prefix.main" "$apultra_prefix.main.apu" >/dev/null
        "$scratch/apultra/apultra" "$apultra_prefix.call" "$apultra_prefix.call.apu" >/dev/null
        "$scratch/apultra/apultra" "$apultra_prefix.jump.transpose" \
            "$apultra_prefix.jump.transpose.apu" >/dev/null
        apultra_tail_bytes=0
        apultra_table_bytes=128
        if (( apultra_tail_length > 0 )); then
            "$apultra_lzma_encoder" "$apultra_tail" "$apultra_tail.lzma" \
                "$apultra_original_length"
            apultra_tail_bytes="$(stat -c %s "$apultra_tail.lzma")"
            apultra_table_bytes=160
        fi
        apultra_payload_bytes="$(($apultra_table_bytes + \
            $(stat -c %s "$apultra_prefix.main.apu") + \
            $(stat -c %s "$apultra_prefix.call.apu") + \
            $(stat -c %s "$apultra_prefix.jump.transpose.apu") + \
            $(stat -c %s "$apultra_prefix.rc") + apultra_tail_bytes))"
        apultra_median="$($scratch/apultra-bcj2-oracle \
            "$apultra_runtime" "$apultra_prefix" 21)"
        IFS=$'\t' read -r apultra_rust_main apultra_rust_call \
            apultra_rust_jump apultra_rust_bcj2 apultra_rust_original_hash <<< "$(
                cargo run --quiet --release --locked \
                    --config 'profile.release.package.packforge-runtime-hash.opt-level="z"' \
                    --config 'profile.release.package.packforge-codec5-decoder.opt-level=2' \
                    -p packforge-codec5-decoder --example m2_codec5_profile -- \
                    "$apultra_runtime" "$apultra_prefix" 21
            )"
        apultra_manifest_size="$(python3 -c \
            'import json,sys; print(json.load(open(sys.argv[1]))["manifest_size"])' \
            "$apultra_inspect")"
        apultra_upx_bytes="$(stat -c %s "$upx_packed")"
        apultra_upx_ceiling="$((apultra_upx_bytes * 105 / 100))"
        apultra_fixed_bytes="$((192 + apultra_manifest_size + 128))"
        apultra_loader_for_upx="$((apultra_upx_bytes - apultra_payload_bytes - apultra_fixed_bytes - 1))"
        apultra_loader_for_ceiling="$((apultra_upx_ceiling - apultra_payload_bytes - apultra_fixed_bytes))"
        apultra_projected_bytes="$((apultra_payload_bytes + apultra_fixed_bytes + apultra_linked_loader_lower_bound))"
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$label" "$apultra_runtime_length" "$apultra_tail_length" \
            "$apultra_payload_bytes" "$apultra_median" \
            "$apultra_rust_main" "$apultra_rust_call" "$apultra_rust_jump" \
            "$apultra_rust_bcj2" "$apultra_rust_original_hash" "$apultra_upx_bytes" \
            "$apultra_upx_ceiling" "$apultra_loader_for_upx" \
            "$apultra_loader_for_ceiling" "$apultra_bare_loader_bytes" \
            "$apultra_oracle_text_bytes" "$apultra_oracle_rodata_bytes" \
            "$apultra_linked_loader_lower_bound" "$apultra_projected_bytes" \
            >> "$apultra_bcj2_output"
    fi

    if [[ -n "$codec5_partition_output" ]]; then
        codec5_loader_bytes="$(stat -c %s "$workspace/runtime/artifacts/linux-x86_64/loader-v2-codec5")"
        while IFS=$'\t' read -r streams main_decoded whole_main split_main main_delta; do
            if [[ "$streams" == "streams" ]]; then
                continue
            fi
            fixed_table_delta=0
            if (( streams == 4 )); then
                fixed_table_delta=96
            elif (( streams == 2 )); then
                fixed_table_delta=48
            fi
            maximum_loader_for_upx="$((apultra_loader_for_upx - main_delta - fixed_table_delta))"
            remaining_loader_bytes="$((maximum_loader_for_upx - codec5_loader_bytes))"
            format_budget_pass=false
            if (( remaining_loader_bytes > 0 )); then
                format_budget_pass=true
            fi
            printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
                "$label" "$streams" "$main_decoded" "$whole_main" "$split_main" \
                "$main_delta" "$fixed_table_delta" "$maximum_loader_for_upx" \
                "$remaining_loader_bytes" "$format_budget_pass" \
                >> "$codec5_partition_output"
        done < <("$codec5_partition_spike" "$apultra_runtime")
    fi

    if [[ -n "$brotli_output" ]]; then
        brotli_payload="$scratch/$label.br"
        "$scratch/brotli-build/brotli" -q 11 -f -o "$brotli_payload" "$original"
        brotli_payload_bytes="$(stat -c %s "$brotli_payload")"
        brotli_median="$($scratch/brotli-decoder-oracle \
            "$brotli_payload" "$original" 21)"
        brotli_manifest_size="$($packer inspect "$packforge" --json | python3 -c \
            'import json,sys; print(json.load(sys.stdin)["manifest_size"])')"
        brotli_upx_ceiling="$(( $(stat -c %s "$upx_packed") * 105 / 100 ))"
        brotli_maximum_loader="$((brotli_upx_ceiling - brotli_payload_bytes - brotli_manifest_size - 192 - 128))"
        brotli_fits=false
        if (( brotli_linked_lower_bound <= brotli_maximum_loader )); then
            brotli_fits=true
        fi
        printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
            "$label" "$brotli_payload_bytes" "$brotli_median" \
            "$brotli_upx_ceiling" "$brotli_manifest_size" \
            "$brotli_maximum_loader" "$brotli_decoder_file_bytes" \
            "$brotli_text_bytes" "$brotli_rodata_bytes" \
            "$brotli_linked_lower_bound" "$brotli_fits" >> "$brotli_output"
    fi

    if [[ -n "$codec_spike_output" ]]; then
        baseline_bytes="$(stat -c %s "$packforge")"
        baseline_payload="$($packer inspect "$packforge" --json | python3 -c \
            'import json,sys; print(json.load(sys.stdin)["payload_size"])')"
        baseline_loader="$($packer inspect "$packforge" --json | python3 -c \
            'import json,sys; print(json.load(sys.stdin)["loader_size"])')"
        upx_bytes="$(stat -c %s "$upx_packed")"
        upx_ceiling="$((upx_bytes * 105 / 100))"
        "$codec_spike" "$original" | tail -n +2 | \
            while IFS=$'\t' read -r streams _ payload_bytes _; do
                runtime_reserve="$((23500 - baseline_loader))"
                projected="$((baseline_bytes - baseline_payload + payload_bytes + runtime_reserve + streams * 32))"
                passes=false
                if (( projected <= upx_ceiling )); then
                    passes=true
                fi
                printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
                    "$label" "$streams" "$payload_bytes" "$projected" \
                    "$upx_ceiling" "$passes" >> "$codec_spike_output"
            done
    fi

    if [[ -n "$asm_oracle_output" ]]; then
        inspect_json="$scratch/$label.oracle-inspect.json"
        "$packer" inspect "$packforge" --json > "$inspect_json"
        read -r loader_size manifest_size payload_size original_size < <(
            python3 -c 'import json,sys; value=json.load(open(sys.argv[1])); print(value["loader_size"], value["manifest_size"], value["payload_size"], value["original_size"])' \
                "$inspect_json"
        )
        codec_tag="$(od -An -tu2 -j "$((loader_size + 12))" -N 2 "$packforge" | tr -d ' ')"
        if [[ "$codec_tag" == "4" ]]; then
            dd if="$packforge" of="$scratch/$label.properties" bs=1 \
                skip="$((loader_size + 20))" count=5 status=none
            dd if="$packforge" of="$scratch/$label.payload" bs=1 \
                skip="$((loader_size + 192 + manifest_size))" count="$payload_size" status=none
            oracle_diagnostic="$($scratch/lzma-codec4-asm-oracle \
                "$scratch/$label.payload" "$scratch/$label.properties" "$original" \
                "$original_size" 21)"
            printf '%s\t%s\t%s\t%s\n' "$label" "$oracle_diagnostic" \
                "$asm_object_bytes" "$asm_text_bytes" >> "$asm_oracle_output"
        fi
    fi

    capture_behavior "$original" "$scratch/original-behavior"
    capture_behavior "$packforge" "$scratch/packforge-behavior"
    capture_behavior "$upx_packed" "$scratch/upx-behavior"
    cmp "$scratch/original-behavior.stdout" "$scratch/packforge-behavior.stdout"
    cmp "$scratch/original-behavior.stderr" "$scratch/packforge-behavior.stderr"
    cmp "$scratch/original-behavior.status" "$scratch/packforge-behavior.status"
    cmp "$scratch/original-behavior.stdout" "$scratch/upx-behavior.stdout"
    cmp "$scratch/original-behavior.stderr" "$scratch/upx-behavior.stderr"
    cmp "$scratch/original-behavior.status" "$scratch/upx-behavior.status"

    if [[ -n "$runtime_traces" ]]; then
        "$packer" inspect "$packforge" --json > "$runtime_traces/$label.inspect.json"
        strace -f -qq -o "$runtime_traces/$label.strace" \
            -e trace=execve,execveat,memfd_create,mmap,mprotect,openat,pread64,write \
            -E PACKFORGE_SMOKE=benchmark \
            "$packforge" round-trip >/dev/null
        if (( phase_iterations > 0 )); then
            phase_traces=()
            for ((phase_iteration = 0; phase_iteration < phase_iterations; phase_iteration++)); do
                phase_trace="$runtime_traces/$label.phase-$phase_iteration.strace"
                strace -f -qq -ttt -T -o "$phase_trace" \
                    -e trace=execve,execveat,memfd_create,mmap,mprotect,openat,pread64,write \
                    -E PACKFORGE_SMOKE=benchmark \
                    "$packforge" round-trip >/dev/null
                phase_traces+=("$phase_trace")
            done
            python3 "$workspace/scripts/runtime_phase_trace.py" \
                "${phase_traces[@]}" \
                --output "$runtime_traces/$label.phases.json"
        fi
    fi

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
