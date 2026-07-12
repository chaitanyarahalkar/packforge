# Packforge

Packforge is a modern executable-packing project focused on predictable behavior,
transparent output, and measurable size/startup tradeoffs.

The packer tool will ship as a single binary per host platform and produce
self-contained executables. The first supported target is Linux ELF on x86-64.

> [!IMPORTANT]
> Packforge's deterministic, reversible `.pfg` container and M2 direct-load
> Linux x86-64 runtime are implemented. Containers remain the default artifact;
> pass `--artifact executable` to produce a self-contained native executable.

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

`pack --artifact executable` produces executable format v2: a native Linux
x86-64 wrapper containing a relocation-free freestanding loader, an authenticated
segment manifest, a bounded raw-LZMA1 payload, and a fixed trailer. `balanced` is
selected automatically. At startup the loader verifies and decompresses the
original, maps its `PT_LOAD` segments with W^X protections, repairs the auxiliary
vector, and transfers directly to the original entry point without a memfd or a
second exec. `inspect`, `verify`, and `unpack` auto-detect containers plus legacy
v1 and direct-load v2 executables.

`pack` produces a checksummed container. `inspect` validates the fixed header and
compressed payload without decompressing. `verify` and `unpack` additionally
reconstruct the executable, validate its length and digest, and reclassify it
against the embedded format metadata.

`benchmark` performs one warm-up and a bounded number of measured iterations for
every stable profile. It checks byte-for-byte deterministic output and reports raw
median/minimum pack and full-verification durations in its JSON schema.

Failures use stable diagnostic identifiers and class-specific exit statuses; see
the [CLI diagnostics contract](docs/DIAGNOSTICS.md). Container JSON output is
defined by a strict [version 1 schema](schemas/container-report-v1.schema.json)
with checked-in inspect and verify examples.

## Packforge vs. UPX

The direct-load v2 benchmark was run on 2026-07-11 using a native GitHub-hosted
Linux x86-64 runner. It compares Packforge's `balanced` raw-LZMA1 executable with
pinned UPX 5.2.0 `--best`. Every artifact passed behavior equivalence first;
timings are medians from 21 warm runs and seven page-cache-reset cold runs.

### Packed size

| Fixture | Packforge v2 | UPX | Packforge/UPX |
| --- | ---: | ---: | ---: |
| C | 296,661 B | 308,912 B | 96.03% |
| C++ | 296,586 B | 308,908 B | 96.01% |
| Rust | 199,330 B | 196,196 B | 101.59% |
| Go | 604,435 B | 653,268 B | 92.52% |

The median fixture ratio is 96.02%, so Packforge wins the aggregate size gate by
3.98%. It is smaller for C, C++, and Go, while the Rust fixture is 1.59% larger;
all four remain inside the 105% per-fixture bound.

### Cold process time and peak RSS

| Fixture | Packforge cold | UPX cold | Packforge RSS | UPX RSS |
| --- | ---: | ---: | ---: | ---: |
| C | 38.08 ms | 9.75 ms | 1,792 KiB | 1,736 KiB |
| C++ | 38.08 ms | 9.80 ms | 1,792 KiB | 1,736 KiB |
| Rust | 25.85 ms | 8.28 ms | 1,736 KiB | 1,736 KiB |
| Go | 75.74 ms | 15.21 ms | 5,864 KiB | 5,816 KiB |

Packforge does **not** beat UPX overall yet: median cold startup is 3.895x UPX,
so the strict M2 release gate fails even though size, RSS, correctness,
reversibility, determinism, W^X direct mapping, and zero-secondary-exec gates
pass. See the [complete evidence](benchmarks/results/m2-linux-x86_64-2026-07-11/README.md),
[method](docs/BENCHMARKING.md), and
[source CI run](https://github.com/chaitanyarahalkar/packforge/actions/runs/29177085582).

## Project boundaries

The first production target is a reversible ELF packer for trusted, first-party
executables. Packforge is not a code protector or an obfuscator. Unsupported
formats or binary features will fail closed without modifying the input.

See the [product plan](docs/PRODUCT.md), [architecture](docs/ARCHITECTURE.md),
[container format](docs/CONTAINER_FORMAT.md), [roadmap](docs/ROADMAP.md),
[segment manifest](docs/MANIFEST_FORMAT.md),
[self-contained executable format](docs/EXECUTABLE_FORMAT.md),
[direct-load executable v2 format](docs/EXECUTABLE_FORMAT_V2.md),
[CLI diagnostics](docs/DIAGNOSTICS.md),
[runtime spike](docs/RUNTIME_SPIKE.md), [benchmark method](docs/BENCHMARKING.md),
[M2 performance plan](docs/plans/M2.md), and [security policy](SECURITY.md).

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p packforge-cli -- status
bash scripts/build-runtime.sh --check
bash scripts/build-runtime-v2.sh --check
```

The current implementation uses BLAKE3 for integrity. Recovery containers support
LZ4 for `fast` and Zstandard for `balanced`/`small`; `auto` deterministically
selects the smallest stable container payload. Direct-load executable v2 uses the
bounded raw-LZMA1 runtime. The measured Zstandard runtime experiment and its no-go
result are documented under `runtime/zstd-spike/`.

## License

MIT.
