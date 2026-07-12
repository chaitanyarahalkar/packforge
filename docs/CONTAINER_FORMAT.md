# Packforge container format v1

This document specifies the M1 `.pfg` recovery container. It is deliberately
small and fixed-width so inspect and runtime implementations can reject malformed
inputs before allocating from attacker-controlled lengths.

All integers use little-endian byte order. Version 1 consists of a 192-byte fixed
header followed immediately by one compressed payload. Trailing data is invalid.

## Header layout

| Offset | Size | Field | Version 1 value or meaning |
| ---: | ---: | --- | --- |
| 0 | 8 | magic | ASCII `PFGCNT01` |
| 8 | 2 | container version | `1` |
| 10 | 2 | header length | `192` |
| 12 | 1 | codec | `1` LZ4 block, `2` Zstandard frame |
| 13 | 1 | requested profile | `1` fast, `2` balanced, `3` small, `4` auto |
| 14 | 1 | executable format | `1` ELF |
| 15 | 1 | executable class | `2` ELF64 |
| 16 | 1 | executable byte order | `1` little-endian |
| 17 | 1 | flags | zero in version 1 |
| 18 | 2 | ELF machine | `62` for x86-64 |
| 20 | 2 | ELF type | `2` for `ET_EXEC` |
| 22 | 2 | load-segment count | nonzero |
| 24 | 4 | original Unix mode | permission and special bits only (`0o7777` mask) |
| 28 | 4 | signed codec level | zero for LZ4; Zstandard level otherwise |
| 32 | 8 | original length | bounded to 1 GiB in M1 |
| 40 | 8 | payload length | bounded to 1 GiB + 64 MiB in M1 |
| 48 | 8 | ELF entry point | copied from the validated ELF header |
| 56 | 32 | configuration digest | BLAKE3 over stable codec/format settings |
| 88 | 32 | original digest | BLAKE3 of the exact input bytes |
| 120 | 32 | payload digest | BLAKE3 of the compressed payload |
| 152 | 32 | header digest | BLAKE3 of the full header with this field zeroed |
| 184 | 8 | reserved | all zero |

## Validation order

Readers must validate in this order:

1. Enforce the complete-file size limit and require at least 192 bytes.
2. Check magic, version, and fixed header length.
3. Verify the header digest before trusting its variable lengths.
4. Reject nonzero flags/reserved bytes and unknown enumerations.
5. Enforce original and payload hard limits using checked conversions.
6. Require the complete file length to equal `192 + payload_length` exactly.
7. Verify the compressed-payload digest.
8. Only for full verification/unpack, decompress with a bounded output.
9. Require the exact original length and digest.
10. Reclassify the reconstructed ELF and compare its format facts with the header.

`inspect` stops after step 7. `verify` and `unpack` perform all steps. None of these
operations executes the original payload.

The LZ4 output buffer uses fallible reservation before initialization, so a host
memory limit becomes a recoverable resource diagnostic rather than an allocator
abort. Zstandard frames are inspected before decoder construction and may declare
at most a 128 MiB window (`windowLog <= 27`), including single-segment frames.
Decoder output is additionally capped to `original_length + 1` while streaming.

Profile/codec combinations are part of the integrity-checked format contract:
`fast` is LZ4 level 0, `balanced` is Zstandard level 3, `small` is Zstandard level
19, and `auto` may contain only one of those three candidates. Other combinations
fail closed even when every digest is recomputed.

## Determinism

Identical input bytes, original mode, Packforge version, and pack options produce
identical output bytes. The current codec selection is single-threaded. The `auto`
profile compares stable candidates and breaks equal-size ties by codec tag and
level.

## Integrity is not authenticity

The three digests detect accidental corruption and inconsistent reconstruction.
They are not signatures: anyone able to rewrite the container can recompute them.
Signed provenance is a future layer and must not be confused with the version 1
integrity checks.

## Compatibility changes

Unknown versions, required flags, codecs, formats, classes, and nonzero reserved
bytes fail closed. A future incompatible layout receives a new container version;
version 1 meanings will not be repurposed.
