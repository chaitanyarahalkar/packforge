#!/usr/bin/env bash
set -euo pipefail

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/packforge-asm-check.XXXXXX")"
trap 'rm -rf "$scratch"' EXIT

seven_zip_commit="f9d78aff31a5f2521ae7ddbdc97c4a8855808959"
asmc_commit="4b669147521b277b9e050922e7c97cb8aa608f45"
source_digest="bddfb31a59c49c8f25f75d19e7330437d2ca3ba81d9655fa427d7585521a3859"
object_digest="3441d63c9e32ed3c89ecc2a79ec1f72c29924ede24b385d1d1d6c32e501962c8"

printf '%s  %s\n' "$source_digest" \
    "$workspace/runtime/third_party/7zip/LzmaDecOpt.asm" | sha256sum --check --status
base64 --decode "$workspace/runtime/third_party/7zip/LzmaDecOpt.o.b64" \
    > "$scratch/checked.o"
printf '%s  %s\n' "$object_digest" "$scratch/checked.o" | sha256sum --check --status

git clone --quiet --no-checkout https://github.com/ip7z/7zip.git "$scratch/7zip"
git -C "$scratch/7zip" checkout --quiet "$seven_zip_commit"
git clone --quiet --no-checkout https://github.com/nidud/asmc.git "$scratch/asmc"
git -C "$scratch/asmc" checkout --quiet "$asmc_commit"
chmod +x "$scratch/asmc/bin/asmc64"
cmp "$workspace/runtime/third_party/7zip/LzmaDecOpt.asm" \
    "$scratch/7zip/Asm/x86/LzmaDecOpt.asm"
(
    cd "$scratch/7zip/Asm/x86"
    "$scratch/asmc/bin/asmc64" -elf64 -DABI_LINUX -Fo"$scratch/" LzmaDecOpt.asm
) >/dev/null
cmp "$scratch/checked.o" "$scratch/LzmaDecOpt.o"
