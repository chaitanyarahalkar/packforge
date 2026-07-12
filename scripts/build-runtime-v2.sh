#!/usr/bin/env bash
set -euo pipefail

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="$workspace/runtime/linux-x86_64"
artifact="$workspace/runtime/artifacts/linux-x86_64/loader-v2"
checksum_file="$artifact.sha256"
mode="${1:---check}"

case "$mode" in
  --check | --update) ;;
  *)
    printf 'usage: %s [--check|--update]\n' "$0" >&2
    exit 2
    ;;
esac

if [[ -n "${CARGO_TARGET_DIR:-}" ]]; then
  target_dir="$CARGO_TARGET_DIR"
  if [[ "$target_dir" != /* ]]; then
    target_dir="$workspace/$target_dir"
  fi
else
  target_dir="$runtime/target/v2"
fi

toolchain="1.97.0"
rustup toolchain install "$toolchain" --profile minimal \
  --target x86_64-unknown-linux-musl \
  --component llvm-tools-preview >/dev/null
cargo_bin="$(rustup which cargo --toolchain "$toolchain")"
rustc_bin="$(rustup which rustc --toolchain "$toolchain")"
sysroot="$("$rustc_bin" --print sysroot)"
host="$("$rustc_bin" -vV | awk '/^host:/ {print $2}')"
objcopy="$sysroot/lib/rustlib/$host/bin/llvm-objcopy"

rustflags='-C linker-flavor=ld.lld -C link-self-contained=no -C link-arg=-nostdlib -C link-arg=-static -C link-arg=-pie -C link-arg=--no-dynamic-linker -C link-arg=-Bsymbolic -C link-arg=--gc-sections -C link-arg=--sort-section=name -C link-arg=--no-eh-frame-hdr -C link-arg=-z -C link-arg=noexecstack -C relocation-model=pic -C force-unwind-tables=no'
(cd "$runtime" && \
  CARGO_TARGET_DIR="$target_dir" RUSTC="$rustc_bin" RUSTFLAGS="$rustflags" \
  "$cargo_bin" build --release --locked --features lzma \
    --bin packforge-runtime-v2-linux-x86-64)

raw_built="$target_dir/x86_64-unknown-linux-musl/release/packforge-runtime-v2-linux-x86-64"
normalized="$(mktemp "${TMPDIR:-/tmp}/packforge-runtime-v2.XXXXXX")"
trap 'rm -f "$normalized"' EXIT
"$objcopy" --remove-section=.comment --remove-section=.eh_frame \
  "$raw_built" "$normalized"

size="$(wc -c < "$normalized" | tr -d ' ')"
if (( size > 23500 )); then
  printf 'runtime v2 artifact is %s bytes; limit is 23500\n' "$size" >&2
  exit 1
fi
python3 "$workspace/scripts/check-runtime-v2-elf.py" "$normalized"

if [[ -n "${PACKFORGE_RUNTIME_V2_OUTPUT:-}" ]]; then
  output="$PACKFORGE_RUNTIME_V2_OUTPUT"
  if [[ "$output" != /* ]]; then
    output="$workspace/$output"
  fi
  mkdir -p "$(dirname "$output")"
  install -m 0644 "$normalized" "$output"
fi

if command -v sha256sum >/dev/null 2>&1; then
  digest="$(sha256sum "$normalized" | awk '{print $1}')"
else
  digest="$(shasum -a 256 "$normalized" | awk '{print $1}')"
fi

if [[ "$mode" == "--update" ]]; then
  install -m 0644 "$normalized" "$artifact"
  printf '%s  loader-v2\n' "$digest" > "$checksum_file"
else
  cmp "$normalized" "$artifact"
  expected="$(awk '{print $1}' "$checksum_file")"
  if [[ "$digest" != "$expected" ]]; then
    printf 'runtime v2 checksum mismatch: expected %s, built %s\n' \
      "$expected" "$digest" >&2
    exit 1
  fi
fi

printf 'runtime v2 artifact verified: %s bytes sha256=%s\n' "$size" "$digest"
