# Roadmap

Milestones are capability gates, not calendar promises. A milestone closes only
when its fixtures, negative tests, benchmarks, and documentation pass.

## M0 — Measurement and format contract

**Status: in progress.** The container/report schema and deterministic codec
selection exist; the reference corpus and UPX benchmark baseline remain.

- Pin the reference UPX version and benchmark environment.
- Build a legal redistributable corpus across Rust, Go, C, and C++.
- Define cold/warm startup, packed size, RSS, and decoder-memory measurements.
- Specify manifest v0 and stable JSON schemas for inspect/verify/benchmark.
- Add fuzz harnesses for manifest decoding before accepting binary payloads.

**Exit:** benchmark harness produces a reproducible baseline and the manifest
round-trips arbitrary bounded segment descriptions.

## M1 — Reversible container

**Status: implementation in progress.** Host-side pack, inspect, verify, and
unpack operations plus negative tests are present. Local formatting, Clippy, and
the 16-test workspace suite pass. The milestone remains open until the real static
ELF CI smoke test and the reference-corpus gates complete.

- Parse and classify ELF headers without mutation.
- Create a non-executable container holding compressed ranges and recovery data.
- Implement inspect, verify, and byte-identical unpack.
- Add deterministic build tests and corruption/decompression-bomb cases.

**Exit:** every supported corpus file round-trips byte-identically; malformed
containers fail closed.

## M2 — Linux ELF x86-64 static executables

**Status: not started.**

- Implement the smallest native runtime stub.
- Support a documented static, non-PIE `ET_EXEC` subset.
- Enforce W^X and payload integrity at runtime.
- Differential-test behavior in isolated runners.
- Ship fast, balanced, small, and auto profiles.

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
