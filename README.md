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

## Packforge vs. UPX

The first native Linux x86-64 benchmark was run on 2026-07-11 using a GitHub-hosted
Ubuntu 24.04.4 runner. It compares Packforge's currently supported executable
`fast` profile with UPX 5.2.0 `--best`. Every artifact executed successfully before
measurement; values are medians from 21 warm runs after one warm-up.

### Packed size

| Fixture | Original | Packforge | Packforge/original | UPX | UPX/original |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 785,304 B | 488,310 B | 62.18% | 308,912 B | 39.33% |
| C++ | 785,304 B | 488,274 B | 62.17% | 308,908 B | 39.33% |
| Rust | 438,400 B | 304,613 B | 69.48% | 196,196 B | 44.75% |
| Go | 1,597,566 B | 1,015,850 B | 63.58% | 653,268 B | 40.89% |

### Warm process time and peak RSS

| Fixture | Original | Packforge | UPX | Packforge RSS | UPX RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| C | 1.97 ms | 11.01 ms | 7.57 ms | 1,736 KiB | 1,736 KiB |
| C++ | 2.02 ms | 11.06 ms | 7.59 ms | 1,736 KiB | 1,736 KiB |
| Rust | 1.86 ms | 7.18 ms | 5.73 ms | 1,736 KiB | 1,736 KiB |
| Go | 2.95 ms | 21.69 ms | 12.88 ms | 3,752 KiB | 3,768 KiB |

On this small corpus, Packforge reduces every original but trails UPX in packed
size and warm startup. These are warm-start microbenchmarks from one hosted runner,
not cold-start or universal performance claims. See the
[complete method](docs/BENCHMARKING.md) and the
[source CI run](https://github.com/chaitanyarahalkar/packforge/actions/runs/29171498154).

## Project boundaries

The first production target is a reversible ELF packer for trusted, first-party
executables. Packforge is not a code protector or an obfuscator. Unsupported
formats or binary features will fail closed without modifying the input.

See the [product plan](docs/PRODUCT.md), [architecture](docs/ARCHITECTURE.md),
[container format](docs/CONTAINER_FORMAT.md), [roadmap](docs/ROADMAP.md),
[segment manifest](docs/MANIFEST_FORMAT.md),
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
