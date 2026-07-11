#!/usr/bin/env bash
set -euo pipefail

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="$workspace/runtime/linux-x86_64"
artifact="$workspace/runtime/artifacts/linux-x86_64/loader-v1"
checksum_file="$artifact.sha256"
mode="${1:---check}"

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
    target_dir="$CARGO_TARGET_DIR"
    if [[ "$target_dir" != /* ]]; then
        target_dir="$workspace/$target_dir"
    fi
    export CARGO_TARGET_DIR="$target_dir"
else
    target_dir="$runtime/target"
fi

case "$mode" in
    --check | --update) ;;
    *)
        printf 'usage: %s [--check|--update]\n' "$0" >&2
        exit 2
        ;;
esac

if command -v rustup >/dev/null 2>&1; then
    toolchain="1.97.0"
    rustup toolchain install "$toolchain" --profile minimal \
        --target x86_64-unknown-linux-musl \
        --component llvm-tools-preview >/dev/null
    cargo_bin="$(rustup which cargo --toolchain "$toolchain")"
    rustc_bin="$(rustup which rustc --toolchain "$toolchain")"
    export RUSTC="$rustc_bin"
    export PATH="$(dirname "$cargo_bin"):$PATH"
    sysroot="$("$rustc_bin" --print sysroot)"
    host="$("$rustc_bin" -vV | awk '/^host:/ {print $2}')"
    objcopy="$sysroot/lib/rustlib/$host/bin/llvm-objcopy"
else
    cargo_bin="$(command -v cargo)"
    if command -v llvm-objcopy >/dev/null 2>&1; then
        objcopy="$(command -v llvm-objcopy)"
    else
        objcopy="$(command -v objcopy)"
    fi
fi

(cd "$runtime" && "$cargo_bin" build --release --locked)
raw_built="$target_dir/x86_64-unknown-linux-musl/release/packforge-runtime-linux-x86-64"
normalized="$(mktemp "${TMPDIR:-/tmp}/packforge-runtime.XXXXXX")"
trap 'rm -f "$normalized"' EXIT
"$objcopy" --remove-section=.comment "$raw_built" "$normalized"
built="$normalized"

size="$(wc -c < "$built" | tr -d ' ')"
if (( size > 32768 )); then
    printf 'runtime artifact is %s bytes; limit is 32768\n' "$size" >&2
    exit 1
fi

file_output="$(file "$built")"
case "$file_output" in
    *"ELF 64-bit"*"x86-64"*"statically linked"*"stripped"*) ;;
    *)
        printf 'unexpected runtime artifact: %s\n' "$file_output" >&2
        exit 1
        ;;
esac

if command -v readelf >/dev/null 2>&1; then
    if readelf -l "$built" | grep -q 'INTERP'; then
        printf 'runtime artifact unexpectedly contains PT_INTERP\n' >&2
        exit 1
    fi
    if readelf -d "$built" 2>&1 | grep -q 'NEEDED'; then
        printf 'runtime artifact unexpectedly has dynamic dependencies\n' >&2
        exit 1
    fi
fi

if command -v sha256sum >/dev/null 2>&1; then
    digest="$(sha256sum "$built" | awk '{print $1}')"
else
    digest="$(shasum -a 256 "$built" | awk '{print $1}')"
fi

if [[ "$mode" == "--update" ]]; then
    install -m 0644 "$built" "$artifact"
    printf '%s  loader-v1\n' "$digest" > "$checksum_file"
else
    cmp "$built" "$artifact"
    expected="$(awk '{print $1}' "$checksum_file")"
    if [[ "$digest" != "$expected" ]]; then
        printf 'runtime checksum mismatch: expected %s, built %s\n' \
            "$expected" "$digest" >&2
        exit 1
    fi
fi

printf 'runtime artifact verified: %s bytes sha256=%s\n' "$size" "$digest"
