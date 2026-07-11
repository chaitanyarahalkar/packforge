# Product plan

## Product statement

Packforge is a self-contained executable packer for software publishers who want
smaller artifacts without accepting opaque mutation, unpredictable compatibility,
or unverifiable runtime behavior.

## Primary user

A release engineer packaging command-line tools or small desktop utilities built
in Rust, Go, C, or C++.

## Jobs to be done

1. Determine whether an executable can be packed safely.
2. Choose an explicit size/startup profile or let Packforge measure candidates.
3. Produce a deterministic, self-contained artifact.
4. Inspect and verify exactly what the packer changed.
5. Recover a byte-identical copy of the original executable.
6. Integrate packing before platform signing in CI.

## Initial supported contract

| Area | Version 0 target |
| --- | --- |
| Host | Linux x86-64; macOS x86-64/ARM64 for the packer CLI |
| Output | Linux ELF x86-64 executable |
| Binary class | Static, non-PIE `ET_EXEC` first |
| Payload | Loadable segments plus reconstruction metadata |
| Codecs | LZ4-class fast profile and Zstandard balanced/small profiles |
| Operations | Pack, unpack, inspect, verify, benchmark |
| Integrity | BLAKE3-class payload and original-image digests |
| Determinism | Byte-identical output for identical inputs and configuration |

Codec and hash crates will be selected only after decoder-size, maintenance, and
licensing evaluation. Names above describe the intended algorithm families, not
locked dependencies.

## Non-goals

- Protecting intellectual property.
- Hiding behavior from analysts or endpoint security products.
- Packing kernel images, drivers, shared libraries, or self-modifying programs in
  the first release.
- Preserving an existing platform signature across packing.
- Claiming universal compatibility.

## What “better” means

Packforge will compare itself against the current UPX release on a versioned,
publicly reproducible corpus. A release may claim an advantage only for a named
profile and supported compatibility tier.

### Release gates

- **Correctness:** 100% pass rate on the supported corpus; no silent corruption.
- **Reversibility:** unpacking produces the exact original-file digest.
- **Determinism:** repeated packing is byte-for-byte identical.
- **Balanced size:** median packed size within 5% of UPX `--best` on the supported
  corpus, including stub overhead.
- **Balanced startup:** lower median cold-start overhead than UPX `--best`, or a
  documented size win if startup is not lower.
- **Safety:** parser, manifest, and decompressor fuzz targets complete the release
  campaign without an unresolved crash or unbounded allocation.
- **Transparency:** every unsupported feature has a stable diagnostic and appears
  in the compatibility report.

These are hypotheses until measured. Failed targets stay visible in release notes.

## User experience principles

- Never destroy the input by default.
- Explain compatibility decisions in format-specific language.
- Show original size, packed size, stub cost, ratio, expected decoder memory, and
  benchmark method.
- Make `inspect` and `verify` safe for automation with stable JSON output.
- Refuse to pack when the output would be larger unless explicitly forced.

