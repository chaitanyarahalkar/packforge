#!/usr/bin/env bash
set -euo pipefail

workspace="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
output_directory="${1:-${TMPDIR:-/tmp}/packforge-runtime-v2-codegen}"

if [[ "$output_directory" != /* ]]; then
  output_directory="$workspace/$output_directory"
fi
mkdir -p "$output_directory"

printf 'scope\topt_level\thash\tstatus\tloader_bytes\tsha256\n'
configurations=(
  'global:z::compact'
  'decoder:s:s:compact'
  'decoder:1:1:compact'
  'decoder:2:2:compact'
  'decoder:3:3:compact'
  'combined:1:1:compact-opt2'
  'combined:2:2:compact-opt1'
  'combined:2:2:compact-opt2'
  'combined:3:3:compact-opt2'
  'global:s::compact'
  'global:1::compact'
  'global:2::compact'
  'global:3::compact'
)
for configuration in "${configurations[@]}"; do
  IFS=: read -r scope opt_level decoder_opt_level hash_implementation <<< "$configuration"
  runtime_opt_level="$opt_level"
  if [[ "$scope" == "decoder" || "$scope" == "combined" ]]; then
    runtime_opt_level=z
  fi
  candidate="$output_directory/loader-v2-$scope-opt-$opt_level-$hash_implementation"
  target_directory="$output_directory/target-$scope-$opt_level-$hash_implementation"
  rm -f "$candidate"
  if result="$({
    CARGO_TARGET_DIR="$target_directory" \
    PACKFORGE_RUNTIME_V2_OPT_LEVEL="$runtime_opt_level" \
    PACKFORGE_RUNTIME_V2_DECODER_OPT_LEVEL="$decoder_opt_level" \
    PACKFORGE_RUNTIME_V2_HASH="$hash_implementation" \
    PACKFORGE_RUNTIME_V2_OUTPUT="$candidate" \
      bash "$workspace/scripts/build-runtime-v2.sh" --candidate
  } 2>&1)"; then
    status=admitted
  else
    status="rejected:$(printf '%s\n' "$result" | tail -n 1 | tr '[:space:]' '_')"
  fi
  if [[ ! -f "$candidate" ]]; then
    printf '%s\t%s\t%s\t%s\t-\t-\n' \
      "$scope" "$opt_level" "$hash_implementation" "$status"
    continue
  fi
  size="$(wc -c < "$candidate" | tr -d ' ')"
  digest="$(printf '%s\n' "$result" | sed -n 's/.*sha256=//p')"
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$scope" "$opt_level" "$hash_implementation" "$status" "$size" "$digest"
done
