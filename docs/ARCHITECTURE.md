# Architecture

## Overview

Packforge separates host-side parsing and planning from the small target-specific
runtime stub embedded in a packed executable.

```text
input ELF
   |
   v
bounded parser -> compatibility classifier -> segment planner
                                           |
                                           v
                               codec/profile optimizer
                                           |
                                           v
runtime stub + manifest + compressed segments + recovery metadata
                                           |
                                           v
                                  packed executable
```

## Workspace shape

- `packforge-cli`: command parsing, human/JSON output, and exit-code contract.
- `packforge-core::format`: bounded ELF classification for the current stable tier.
- `packforge-core::container`: versioned header, codecs, integrity, reconstruction,
  and deterministic profile selection.
- `packforge-core` host operations: bounded reads and atomic no-clobber writes.
- Future `packforge-runtime`: target-specific loader logic shared by generated stubs.
- `runtime/`: target-specific loader stubs built independently and embedded as
  versioned artifacts.

## Packed artifact model

The output remains a valid native executable. A minimal loader stub starts first,
validates the manifest, reserves memory, reconstructs original load segments,
applies the permitted relocation subset, finalizes memory permissions, clears
temporary state, and transfers control to the original entry point.

The M1 container is deliberately non-executable: it proves deterministic packing,
inspection, verification, and byte-identical recovery before runtime code is
trusted. The initial M2 static `ET_EXEC` runtime tier deliberately avoids
dynamic-linker emulation.
PIE and dynamically linked executables are separate compatibility tiers because
their relocation, TLS, loader, and security-property requirements are materially
different.

## Manifest v0 design constraints

The manifest will contain:

- format magic and independently versioned schema;
- target architecture and compatibility tier;
- original file length and digest;
- bounded segment descriptors;
- codec identifiers and decoder-memory limits;
- compressed ranges and uncompressed lengths;
- original entry point and required memory permissions;
- packer version and deterministic configuration digest;
- optional provenance metadata that is excluded from runtime trust decisions.

Every length, count, address, and sum is validated before allocation or mapping.
Unknown required fields fail closed. The manifest has no scripting facility.

## Runtime rules

- No filesystem extraction in native mode.
- No network access.
- No environment-controlled decoder configuration.
- No RWX mapping.
- No loading code from outside the packed artifact.
- No fallback execution of the compressed input after verification failure.
- Control transfers only after all payload digests and permissions validate.

## Two-stage optimization

1. The segment planner determines what can be compressed without changing the
   declared compatibility tier.
2. The profile optimizer evaluates codec candidates including stub size, compressed
   size, decoder memory, and measured startup overhead.

The `auto` profile minimizes a documented score rather than silently choosing the
smallest file. Its weights and measurements are emitted in JSON.

## Compatibility strategy

Each format feature is classified as supported, rejected, or experimental.
Packforge records the classification report before writing output. Experimental
features require an explicit flag and never count toward stable compatibility
claims.

Differential tests execute original and packed fixtures in isolated test runners
and compare exit status, stdout, stderr, signals, file effects, and a declared
environment snapshot. Resource usage is measured separately from correctness.

## Platform expansion

- ELF ARM64 reuses the container and host plan but receives its own runtime stub.
- PE adds imports, relocations, TLS callbacks, resources, CFG/CET, and Authenticode
  workflow requirements.
- Mach-O is last because code signing, notarization, hardened runtime behavior,
  universal binaries, chained fixups, and OS drift require a separate design.
