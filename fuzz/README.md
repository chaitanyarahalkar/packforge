# Packforge fuzz targets

The fuzz package is intentionally separate from the release workspace. Run the
bounded parser and runtime primitive campaigns with `cargo-fuzz`:

```bash
cargo fuzz run artifact_parsers -- -max_len=1048576
cargo fuzz run manifest_parser -- -max_len=8192
cargo fuzz run runtime_lz4 -- -max_len=1048576
cargo fuzz run runtime_hash -- -max_len=1048576
```

`artifact_parsers` covers standalone container and executable-wrapper framing at
both inspection and full-verification depth. `runtime_lz4` caps attacker-selected
output allocation at 1 MiB inside the harness. `manifest_parser` checks that every
accepted manifest has one canonical re-encoding. `runtime_hash` differentially
compares the compact freestanding implementation with the pinned BLAKE3 crate.

Release campaigns store minimized crashing inputs as regression fixtures before
the associated issue is considered fixed. Corpus and crash directories remain
local unless a minimized input is intentionally committed.
