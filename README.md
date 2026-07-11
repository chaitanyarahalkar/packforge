# Packforge

Packforge is a modern executable-packing project focused on predictable behavior,
transparent output, and measurable size/startup tradeoffs.

The packer tool will ship as a single binary per host platform and produce
self-contained executables. The first supported target is Linux ELF on x86-64.

> [!IMPORTANT]
> Packforge's deterministic, reversible `.pfg` container is implemented. The M2
> Linux x86-64 native runtime is available as an opt-in compatibility spike and
> is not yet the default output.

## Why another packer?

Packforge is not trying to replace decades of format support on day one. It is
designed around a narrower modern contract:

- deterministic, reproducible packed output;
- safe, bounded parsing and decompression;
- explicit compatibility tiers instead of best-effort mutation;
- automatic size-versus-startup optimization;
- reversible and inspectable packaging;
- signing-aware workflows and provenance metadata;
- no encryption, anti-debugging, polymorphism, or evasion behavior.

The word "better" is treated as a benchmarkable claim. Packforge will publish a
corpus, measurement method, failures, and comparisons instead of relying on a
single compression-ratio number.

## CLI

```text
packforge pack [--artifact container|executable] [--profile fast|balanced|small|auto] <input> [-o <output>]
packforge unpack <input> [-o <output>]
packforge inspect [--json] <input>
packforge verify [--json] <input>
packforge benchmark [--iterations 5] [--json] <input>
```

M1 accepts static, non-PIE, little-endian ELF64 x86-64 executables. It validates
the ELF program-header table and rejects `PT_INTERP` and `PT_DYNAMIC` instead of
silently claiming dynamic executables are supported.

`pack --artifact executable --profile fast` produces a native Linux x86-64
wrapper containing the freestanding loader, an embedded recovery container, and
an integrity-checked fixed trailer. During the runtime spike, executable output is
LZ4-only and remains opt-in; `fast` is selected automatically when `--profile`
is omitted. `inspect`, `verify`, and `unpack` auto-detect both artifact kinds.

`pack` produces a checksummed container. `inspect` validates the fixed header and
compressed payload without decompressing. `verify` and `unpack` additionally
reconstruct the executable, validate its length and digest, and reclassify it
against the embedded format metadata.

`benchmark` performs one warm-up and a bounded number of measured iterations for
every stable profile. It checks byte-for-byte deterministic output and reports raw
median/minimum pack and full-verification durations in its JSON schema.

## Project boundaries

The first production target is a reversible ELF packer for trusted, first-party
executables. Packforge is not a code protector or an obfuscator. Unsupported
formats or binary features will fail closed without modifying the input.

See the [product plan](docs/PRODUCT.md), [architecture](docs/ARCHITECTURE.md),
[container format](docs/CONTAINER_FORMAT.md), [roadmap](docs/ROADMAP.md),
[self-contained executable format](docs/EXECUTABLE_FORMAT.md),
[runtime spike](docs/RUNTIME_SPIKE.md), [benchmark method](docs/BENCHMARKING.md),
and [security policy](SECURITY.md).

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p packforge-cli -- status
bash scripts/build-runtime.sh --check
```

The current implementation uses BLAKE3 for integrity. Recovery containers support
LZ4 for `fast` and Zstandard for `balanced`/`small`; `auto` deterministically
selects the smallest stable container payload. Self-contained executable output
currently uses only the bounded LZ4 runtime. The measured Zstandard runtime
experiment and its no-go result are documented under `runtime/zstd-spike/`.

## License

MIT.
