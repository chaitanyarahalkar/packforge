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

### M2 size candidate

The admitted raw-LZMA1 candidate now passes the pre-integration size gate on the
same four fixtures. This projection includes the complete 14,776-byte
feature-retained loader, the 192-byte v2 image header, actual manifest v0 size,
and the 128-byte trailer.

| Fixture | Projected Packforge v2 | UPX 5.2.0 `--best` | Packforge/UPX |
| --- | ---: | ---: | ---: |
| C | 294,077 B | 308,912 B | 95.19% |
| C++ | 294,002 B | 308,908 B | 95.17% |
| Rust | 196,746 B | 196,196 B | 100.28% |
| Go | 601,851 B | 653,268 B | 92.13% |

Median projected size is 95.18% of UPX and every fixture is below the 105% hard
bound. This is a reproducible feasibility result, not a release benchmark: v2
must still integrate the direct loader and beat UPX in measured cold startup.
See the [native gate run](https://github.com/chaitanyarahalkar/packforge/actions/runs/29174191514).

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
