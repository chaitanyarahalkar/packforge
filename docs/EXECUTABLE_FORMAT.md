# Packforge self-contained executable format v1

This document specifies the first Linux x86-64 self-contained Packforge
artifact. It is an executable wrapper around the existing version 1 recovery
container; it does not introduce a second compression or recovery format.

```text
+--------------------------+  offset 0
| native loader ELF        |
+--------------------------+  container_offset
| PFGCNT01 container v1    |
+--------------------------+
| PFGEXE01 trailer v1      |  fixed 128 bytes
+--------------------------+  exact end of file
```

Appending the container and trailer does not change the loader ELF program
headers. The Linux kernel loads the prefix normally, and the loader reads and
validates the appended data through `/proc/self/exe`.

## Stable compatibility tier

- Target: Linux ELF64 x86-64.
- Payload: static, non-PIE `ET_EXEC` accepted by the existing classifier.
- Runtime codec: LZ4 block format.
- Runtime ABI: version 1.
- Minimum experimental kernel: Linux 3.19, subject to the `memfd_create` and
  `execveat` behavior documented in `RUNTIME_SPIKE.md`.

Zstandard containers remain valid recovery containers, but executable wrappers
must reject them until a bounded runtime decoder passes the size and startup
gates. Unknown versions, ABIs, targets, flags, or reserved fields fail closed.

## Trailer v1

All integers use little-endian byte order. The trailer is the final 128 bytes of
the file.

| Offset | Size | Field | Version 1 value or meaning |
| ---: | ---: | --- | --- |
| 0 | 8 | magic | ASCII `PFGEXE01` |
| 8 | 2 | executable format version | `1` |
| 10 | 2 | trailer length | `128` |
| 12 | 2 | runtime ABI | `1` |
| 14 | 2 | flags | zero |
| 16 | 8 | container offset | exact loader length |
| 24 | 8 | container length | complete embedded PFG container |
| 32 | 8 | executable length | exact complete file length |
| 40 | 8 | loader length | equal to container offset |
| 48 | 32 | loader digest | BLAKE3 of bytes before the container |
| 80 | 2 | container version | `1` |
| 82 | 2 | target operating system | `1` for Linux |
| 84 | 2 | target architecture | ELF machine `62` for x86-64 |
| 86 | 10 | reserved | all zero |
| 96 | 32 | trailer digest | BLAKE3 of the trailer with this field zeroed |

The loader digest is useful to host-side verification and reproducible-artifact
checks. It is an integrity value, not an authenticity mechanism or a defense
against deliberate replacement of both the loader and its metadata.

## Validation order

Host readers and the runtime must validate in this order:

1. Enforce the complete executable hard limit and require at least 128 bytes.
2. Read exactly the final 128 bytes and check magic, version, and length.
3. Verify the trailer digest before trusting its variable offsets or lengths.
4. Reject unknown ABI, flags, target values, and nonzero reserved bytes.
5. Require the declared executable length to equal the actual file length.
6. Require a nonempty loader within the fixed stub-size budget.
7. Require `loader_length == container_offset`.
8. Using checked arithmetic, require the container to end exactly where the
   trailer begins and to remain within the container hard limit.
9. Verify the loader digest and its static ELF64 x86-64 classification.
10. Apply the PFG container validation order from `CONTAINER_FORMAT.md`.
11. Before execution, decompress with an exact bounded output, verify the
    original digest, create and seal the anonymous executable file, and only
    then call `execveat` or its diskless `/proc/self/fd/<fd>` `execve` fallback.

Inspection and unpacking never execute the payload. There is no disk extraction,
network access, environment-selected decoder configuration, or fallback after a
validation failure.

## Determinism and reversibility

For fixed input bytes, input mode, pack options, loader bytes, and format
versions, the complete executable is byte-for-byte deterministic. Unpacking the
embedded PFG container recovers the original executable bytes and mode exactly.

The complete wrapper size, including loader and trailer overhead, is used when
deciding whether packing is beneficial. A larger result requires the explicit
allow-larger option.

## CLI migration

During the runtime spike, native output is opt-in through
`pack --artifact executable`; existing container output remains the default.
`inspect`, `verify`, and `unpack` will auto-detect both formats. After the runtime
correctness, compatibility, security, and performance gates pass, executable
output becomes the default while `--artifact container` remains available for
recovery-only workflows.
