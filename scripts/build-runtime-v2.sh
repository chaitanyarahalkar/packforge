#!/usr/bin/env bash
set -euo pipefail

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runtime="$workspace/runtime/linux-x86_64"
mode="${1:---check}"
opt_level="${PACKFORGE_RUNTIME_V2_OPT_LEVEL:-z}"
relocation_model="${PACKFORGE_RUNTIME_V2_RELOCATION_MODEL:-pic}"
decoder_opt_level="${PACKFORGE_RUNTIME_V2_DECODER_OPT_LEVEL-3}"
decoder_implementation="${PACKFORGE_RUNTIME_V2_DECODER:-parallel}"
hash_implementation="${PACKFORGE_RUNTIME_V2_HASH:-compact-opt2}"
artifact_name="loader-v2"
size_limit=23500
if [[ "$decoder_implementation" == "apultra-bcj2" ]]; then
  artifact_name="loader-v2-codec5"
  size_limit=15701
fi
artifact="$workspace/runtime/artifacts/linux-x86_64/$artifact_name"
checksum_file="$artifact.sha256"

case "$mode" in
  --check | --update | --candidate) ;;
  *)
    printf 'usage: %s [--check|--update|--candidate]\n' "$0" >&2
    exit 2
    ;;
esac

case "$opt_level" in
  z | s | 1 | 2 | 3) ;;
  *)
    printf 'PACKFORGE_RUNTIME_V2_OPT_LEVEL must be one of: z, s, 1, 2, 3\n' >&2
    exit 2
    ;;
esac

case "$relocation_model" in
  pic | pie) ;;
  *)
    printf 'PACKFORGE_RUNTIME_V2_RELOCATION_MODEL must be pic or pie\n' >&2
    exit 2
    ;;
esac

if [[ -n "$decoder_opt_level" ]]; then
  case "$decoder_opt_level" in
    z | s | 1 | 2 | 3) ;;
    *)
      printf 'PACKFORGE_RUNTIME_V2_DECODER_OPT_LEVEL must be one of: z, s, 1, 2, 3\n' >&2
      exit 2
      ;;
  esac
fi

case "$hash_implementation" in
  compact) runtime_features=lzma ;;
  compact-optz | compact-opt1 | compact-opt2) runtime_features=lzma,optimized-hash ;;
  *)
    printf 'PACKFORGE_RUNTIME_V2_HASH must be compact, compact-optz, compact-opt1, or compact-opt2\n' >&2
    exit 2
    ;;
esac
runtime_features="runtime-v2,$runtime_features"

case "$decoder_implementation" in
  none) runtime_features="${runtime_features/lzma,/}" ;;
  rust) ;;
  asm) runtime_features="${runtime_features/lzma/lzma-asm}" ;;
  parallel) runtime_features="${runtime_features/lzma/lzma-parallel}" ;;
  apultra-bcj2) runtime_features="${runtime_features/lzma/apultra-bcj2}" ;;
  *)
    printf 'PACKFORGE_RUNTIME_V2_DECODER must be none, rust, asm, parallel, or apultra-bcj2\n' >&2
    exit 2
    ;;
esac

if [[ "$mode" == "--candidate" && -z "${PACKFORGE_RUNTIME_V2_OUTPUT:-}" ]]; then
  printf 'PACKFORGE_RUNTIME_V2_OUTPUT is required with --candidate\n' >&2
  exit 2
fi

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

rustflags="-C linker-flavor=ld.lld -C link-self-contained=no -C link-arg=-nostdlib -C link-arg=-static -C link-arg=-pie -C link-arg=--no-dynamic-linker -C link-arg=-Bsymbolic -C link-arg=--gc-sections -C link-arg=--icf=all -C link-arg=--sort-section=name -C link-arg=--no-eh-frame-hdr -C link-arg=-z -C link-arg=noexecstack -C relocation-model=$relocation_model -C force-unwind-tables=no"
if [[ "$decoder_implementation" == "asm" || "$decoder_implementation" == "parallel" ]]; then
  asm_object="$target_dir/LzmaDecOpt.o"
  mkdir -p "$target_dir"
  if ! base64 --decode "$workspace/runtime/third_party/7zip/LzmaDecOpt.o.b64" > "$asm_object" 2>/dev/null; then
    base64 -D -i "$workspace/runtime/third_party/7zip/LzmaDecOpt.o.b64" -o "$asm_object"
  fi
  actual_asm_digest="$(shasum -a 256 "$asm_object" | awk '{print $1}')"
  expected_asm_digest="3441d63c9e32ed3c89ecc2a79ec1f72c29924ede24b385d1d1d6c32e501962c8"
  if [[ "$actual_asm_digest" != "$expected_asm_digest" ]]; then
    printf 'runtime v2 assembly object checksum mismatch\n' >&2
    exit 1
  fi
  asm_link_object="$target_dir/LzmaDecOpt.hidden.o"
  "$objcopy" --set-symbol-visibility=LzmaDec_DecodeReal_3=hidden \
    "$asm_object" "$asm_link_object"
  rustflags="$rustflags -C link-arg=$asm_link_object"
fi
cargo_arguments=(
  build --release --locked --features "$runtime_features"
  --bin packforge-runtime-v2-linux-x86-64
)
if [[ -n "$decoder_opt_level" ]]; then
  decoder_opt_value="$decoder_opt_level"
  if [[ "$decoder_opt_level" == "z" || "$decoder_opt_level" == "s" ]]; then
    decoder_opt_value="\"$decoder_opt_level\""
  fi
  decoder_package="packforge-lzma-decoder"
  if [[ "$decoder_implementation" == "apultra-bcj2" ]]; then
    decoder_package="packforge-codec5-decoder"
  fi
  cargo_arguments=(
    --config "profile.release.package.$decoder_package.opt-level=$decoder_opt_value"
    "${cargo_arguments[@]}"
  )
fi
if [[ "$hash_implementation" == compact-opt* ]]; then
  hash_opt_level="${hash_implementation#compact-opt}"
  if [[ "$hash_opt_level" == "z" ]]; then
    hash_opt_level='"z"'
  fi
  cargo_arguments=(
    --config "profile.release.package.packforge-runtime-hash.opt-level=$hash_opt_level"
    "${cargo_arguments[@]}"
  )
fi
(cd "$runtime" && \
  CARGO_TARGET_DIR="$target_dir" RUSTC="$rustc_bin" RUSTFLAGS="$rustflags" \
  CARGO_PROFILE_RELEASE_OPT_LEVEL="$opt_level" \
  "$cargo_bin" "${cargo_arguments[@]}")

raw_built="$target_dir/x86_64-unknown-linux-musl/release/packforge-runtime-v2-linux-x86-64"
normalized="$(mktemp "${TMPDIR:-/tmp}/packforge-runtime-v2.XXXXXX")"
trap 'rm -f "$normalized"' EXIT
"$objcopy" --remove-section=.comment --remove-section=.eh_frame \
  "$raw_built" "$normalized"

size="$(wc -c < "$normalized" | tr -d ' ')"
if (( size > size_limit )); then
  printf 'runtime v2 artifact is %s bytes; limit is %s\n' "$size" "$size_limit" >&2
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
  printf '%s  %s\n' "$digest" "$artifact_name" > "$checksum_file"
elif [[ "$mode" == "--check" ]]; then
  cmp "$normalized" "$artifact"
  expected="$(awk '{print $1}' "$checksum_file")"
  if [[ "$digest" != "$expected" ]]; then
    printf 'runtime v2 checksum mismatch: expected %s, built %s\n' \
      "$expected" "$digest" >&2
    exit 1
  fi
fi

printf 'runtime v2 artifact verified: opt=%s decoder=%s decoder-opt=%s hash=%s relocation=%s size=%s bytes sha256=%s\n' \
  "$opt_level" "$decoder_implementation" "${decoder_opt_level:-inherit}" "$hash_implementation" \
  "$relocation_model" "$size" "$digest"
