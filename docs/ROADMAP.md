# Roadmap

Milestones are capability gates, not calendar promises. A milestone closes only
when its fixtures, negative tests, benchmarks, and documentation pass.

## M0 — Measurement and format contract

**Status: in progress.** The container/report schema, deterministic codec selection,
bounded per-profile benchmark command, C/C++/Rust/Go executable corpus, and pinned
UPX 5.2.0 baseline exist. Cold-start infrastructure and completed release
campaigns remain.

- Keep the reference UPX version, asset digest, and benchmark environment pinned.
- Build a legal redistributable corpus across Rust, Go, C, and C++.
- Define cold/warm startup, packed size, RSS, and decoder-memory measurements.
- Specify manifest v0 and stable JSON schemas for inspect/verify/benchmark.
- Add fuzz harnesses for manifest decoding before accepting binary payloads.

The repository now includes separate fuzz targets for container/executable
parsers, the freestanding LZ4 decoder, and differential BLAKE3 validation. M0
remains open until the versioned release campaigns complete without an unresolved
failure.

**Exit:** benchmark harness produces a reproducible baseline and the manifest
round-trips arbitrary bounded segment descriptions.

## M1 — Reversible container

**Status: implementation in progress.** Host-side pack, inspect, verify, and
unpack operations plus negative tests are present. Local formatting, Clippy, the
workspace suite, and the real static-ELF round trip pass. The milestone remains
open until the reference-corpus and fuzz-campaign gates complete.

- Parse and classify ELF headers without mutation.
- Create a non-executable container holding compressed ranges and recovery data.
- Implement inspect, verify, and byte-identical unpack.
- Add deterministic build tests and corruption/decompression-bomb cases.

**Exit:** every supported corpus file round-trips byte-identically; malformed
containers fail closed.

## M2 — Linux ELF x86-64 static executables

**Status: runtime spike in progress.** The fixed executable trailer, host-side
wrapping/inspection/verification/recovery path, freestanding LZ4 runtime, and
native Linux CI smoke gate are implemented. Static C, C++, Rust/musl, and Go
fixtures pass byte-identical recovery and execution equivalence; the behavioral
fixture also covers cwd, inherited descriptors, output, file effects, status, and
signals. Native kernel-matrix and performance gates remain before executable
output becomes the default. The `ruzstd` 0.8.3 experiment decoded the balanced
payload correctly but added 59,936 bytes over its static I/O baseline, so it was
rejected for the 32 KiB M2 loader budget rather than integrated.

- Implement the smallest native runtime stub.
- Support a documented static, non-PIE `ET_EXEC` subset.
- Enforce W^X and payload integrity at runtime.
- Differential-test behavior in isolated runners.
- Ship fast first; balanced, small, and auto executable profiles require a
  bounded decoder that passes the same runtime size gate.

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
