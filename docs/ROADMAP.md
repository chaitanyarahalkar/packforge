# Roadmap

Milestones are capability gates, not calendar promises. A milestone closes only
when its fixtures, negative tests, benchmarks, and documentation pass.

## M0 — Measurement and format contract

**Status: complete.** Corpus v1, manifest v0, the benchmark-report schema, frozen
environment metadata, raw warm/cold/RSS samples, deterministic aggregation, and
the pinned UPX 5.2.0 evaluator are implemented. The first native baseline and a
four-target sanitizer campaign are linked from `plans/M0.md`; current UPX
performance gates fail explicitly and feed M2 optimization.

- Keep the reference UPX version, asset digest, and benchmark environment pinned.
- Build a legal redistributable corpus across Rust, Go, C, and C++.
- Define cold/warm startup, packed size, RSS, and decoder-memory measurements.
- Specify manifest v0 and stable JSON schemas for inspect/verify/benchmark.
- Add fuzz harnesses for manifest decoding before accepting binary payloads.

The repository includes separate fuzz targets for container/executable parsers,
manifest v0, the freestanding LZ4 decoder, and differential BLAKE3 validation.

**Exit:** benchmark harness produces a reproducible baseline and the manifest
round-trips arbitrary bounded segment descriptions.

## M1 — Reversible container

**Status: complete.** Host-side pack, inspect, verify, and unpack operations pass
the four-language/four-profile corpus under a 512 MiB limit. Strict JSON goldens,
stable diagnostics, exhaustive malformed-header regressions, Linux/macOS
byte-identical output, five fuzz targets, and zero-warning dependency audits are
linked from `plans/M1.md`.

- Parse and classify ELF headers without mutation.
- Create a non-executable container holding compressed ranges and recovery data.
- Implement inspect, verify, and byte-identical unpack.
- Add deterministic build tests and corruption/decompression-bomb cases.

**Exit:** every supported corpus file round-trips byte-identically; malformed
containers fail closed.

## M2 — Linux ELF x86-64 static executables

**Status: in progress; performance gate failed.** Executable v2 now ships a
17,360-byte relocation-free `ET_DYN` loader with authenticated raw-LZMA1 framing,
canonical manifest verification, collision-safe W^X `PT_LOAD` mapping, auxv
repair, and direct entry transfer. Static C, C++, Rust/musl, Go, and process
semantics pass on Ubuntu 22.04 and 24.04 runner families with full ASLR. Syscall
evidence contains one initial exec and no memfd or secondary exec.

The final native campaign beats pinned UPX 5.2.0 `--best` on median packed size
(96.02% of UPX) and passes the per-fixture size, RSS, correctness, reversibility,
determinism, and direct-mapping gates. It does not close M2: median cold startup
is 389.50% of UPX. The exact failing result is preserved under
`benchmarks/results/m2-linux-x86_64-2026-07-11/` and must guide the next decoder
and verification-path optimization work.

- Implement the smallest native runtime stub.
- Support a documented static, non-PIE `ET_EXEC` subset.
- Enforce W^X and payload integrity at runtime.
- Differential-test behavior in isolated runners.
- Optimize the balanced LZMA1 startup path without weakening verification,
  deterministic output, direct mapping, or the loader size bound.

**Exit:** all stable-tier fixtures execute equivalently and meet the M2 release
gates in `PRODUCT.md`.

## M3 — Linux ELF x86-64 PIE and dynamic linking

**Status: not started.**

- Add the required relocation subset and position independence.
- Preserve TLS, auxiliary-vector expectations, and loader-visible metadata.
- Test musl and glibc families independently.
- Add CET-aware fixtures and explicit unsupported diagnostics.

**Exit:** supported PIE/dynamic fixtures pass differential testing across the
pinned Linux distribution matrix.

## M4 — Linux ELF ARM64

**Status: not started.**

- Add an ARM64 runtime stub and relocation support.
- Preserve BTI/PAC-related properties where applicable.
- Run native and emulated cross-architecture verification.

**Exit:** ARM64 reaches the same stable compatibility tier and reporting quality as
x86-64.

## M5 — Windows PE x64 and ARM64

**Status: not started.**

- Design PE-specific manifest fields and loader behavior.
- Support imports, base relocations, TLS callbacks, resources, CFG, and CET in
  explicit tiers.
- Document pack-before-Authenticode-signing CI.

**Exit:** signed post-pack samples install and run on the pinned Windows matrix.

## M6 — Mach-O feasibility

**Status: not started.**

- Prototype x86-64 and ARM64 loaders without claiming stable support.
- Test universal binaries, chained fixups, hardened runtime, signing, and
  notarization workflows.
- Publish a go/no-go report before productizing support.

**Exit:** proceed only if the runtime can preserve platform security properties
without fragile OS-version assumptions.

## Continuous workstreams

- Parser and decompressor fuzzing.
- Corpus growth and regression minimization.
- Reproducible release builds and SBOM/provenance output.
- Antivirus false-positive reporting without evasive countermeasures.
- Benchmark publication and honest comparison notes.
