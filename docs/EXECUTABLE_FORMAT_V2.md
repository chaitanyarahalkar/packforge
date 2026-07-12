# Packforge self-contained executable format v2

Executable v2 is the M2 direct-load format for static, non-PIE Linux ELF64
x86-64 inputs. It is independently versioned from the recovery-only container
format and remains byte-for-byte reversible.

```text
+--------------------------+  offset 0
| position-independent     |
| native loader ELF        |
+--------------------------+  image_offset == loader_length
| PFGIMG02 image header    |  fixed 192 bytes
+--------------------------+
| PFGMAN00 manifest v0     |  40 + 48 * PT_LOAD count
+--------------------------+
| raw LZMA1 payload        |  complete original executable
+--------------------------+
| PFGEXE02 trailer v2      |  fixed 128 bytes
+--------------------------+  exact end of file
```

The manifest and compressed payload immediately follow the image header; their
offsets are derived rather than redundantly encoded. All additions use checked
arithmetic and the payload must end exactly where the final trailer begins.

## Stable compatibility tier

- Target: Linux ELF64 x86-64 static `ET_EXEC`, with no `PT_INTERP` or
  `PT_DYNAMIC`.
- Runtime codec: raw LZMA1, fixed `lc=3`, `lp=0`, `pb=2`, no end marker, and a
  4 KiB through 64 MiB dictionary.
- Manifest: canonical Packforge manifest v0 derived from every `PT_LOAD` program
  header in original order.
- Runtime ABI: version 2.
- Complete loader budget: 23,500 bytes.

Executable v1 remains readable and exactly unpackable. A v2 reader never treats
v1 fields as v2 fields, and unknown magic, versions, ABIs, codecs, targets,
flags, or reserved bytes fail closed.

## Image header v2

All integers use little-endian byte order. The image header begins immediately
after the loader and is exactly 192 bytes.

| Offset | Size | Field | Version 2 value or meaning |
| ---: | ---: | --- | --- |
| 0 | 8 | magic | ASCII `PFGIMG02` |
| 8 | 2 | image format version | `2` |
| 10 | 2 | image header length | `192` |
| 12 | 2 | codec | `3` for raw LZMA1 |
| 14 | 2 | flags | zero |
| 16 | 4 | original Unix mode | permission and executable bits restored by host unpack |
| 20 | 5 | LZMA properties | property byte plus little-endian dictionary size |
| 25 | 1 | expected trailing bytes | range-coder flush bytes, `0` through `5` |
| 26 | 6 | reserved | zero |
| 32 | 8 | manifest length | exact canonical manifest v0 length |
| 40 | 8 | payload length | exact raw LZMA1 byte length |
| 48 | 8 | original length | exact decompressed executable length |
| 56 | 8 | reserved | zero |
| 64 | 32 | original digest | BLAKE3 of the complete original executable |
| 96 | 32 | manifest digest | BLAKE3 of the exact manifest bytes |
| 128 | 32 | payload digest | BLAKE3 of the exact compressed payload bytes |
| 160 | 32 | header digest | BLAKE3 of this header with this field zeroed |

Codec tag `3` is reserved for Packforge's fixed raw-LZMA1 profile; it does not
change the existing container-v1 tags for LZ4 and Zstandard.

## Trailer v2

The final 128 bytes retain the bounded v1 trailer shape but use v2 magic and
semantics.

| Offset | Size | Field | Version 2 value or meaning |
| ---: | ---: | --- | --- |
| 0 | 8 | magic | ASCII `PFGEXE02` |
| 8 | 2 | executable format version | `2` |
| 10 | 2 | trailer length | `128` |
| 12 | 2 | runtime ABI | `2` |
| 14 | 2 | flags | zero |
| 16 | 8 | image offset | exact loader length |
| 24 | 8 | image length | header + manifest + payload |
| 32 | 8 | executable length | exact complete file length |
| 40 | 8 | loader length | equal to image offset |
| 48 | 32 | loader digest | BLAKE3 of bytes before the image header |
| 80 | 2 | image format version | `2` |
| 82 | 2 | target operating system | `1` for Linux |
| 84 | 2 | target architecture | ELF machine `62` for x86-64 |
| 86 | 10 | reserved | zero |
| 96 | 32 | trailer digest | BLAKE3 of the trailer with this field zeroed |

## Host validation order

1. Enforce the complete executable hard limit and require the 128-byte trailer.
2. Dispatch only on exact trailer magic, then validate version and fixed length.
3. Verify the trailer digest before trusting any variable length.
4. Reject unknown ABI, flags, image version, target, and reserved bytes.
5. Require declared and actual executable lengths to match.
6. Require `image_offset == loader_length`, a nonempty loader no larger than
   23,500 bytes, and `loader + image + trailer == executable length` exactly.
7. Verify the loader digest and the loader's static ELF64 x86-64 structure.
8. Read the fixed image header, verify its magic/version/length and digest, then
   reject unknown codec data, flags, properties, or reserved bytes.
9. Derive the manifest and payload ranges using checked arithmetic and require
   the payload to end exactly at the trailer.
10. Verify both range digests before decoding either range.
11. Decode and canonically re-encode manifest v0; require its original size to
    equal the image header and its segment count to match the source ELF.
12. Decode LZMA into an exact bounded output; require the decoder's trailing-byte
    count, output digest, and output length to match the header.
13. Re-parse the recovered ELF and require its canonical manifest to equal the
    embedded manifest byte-for-byte before reporting full verification.

Inspection stops after step 11 and never decompresses or executes the payload.
Verification and unpacking perform all steps. There is no permissive codec
fallback after a validation failure.

## Runtime validation and transfer

The runtime repeats all framing, digest, property, manifest, and decoded-output
checks needed for safe execution. It maps only page-rounded `PT_LOAD` ranges at
their declared addresses, rejects collisions and overlapping manifest ranges,
copies initialized bytes, zeroes `p_memsz - p_filesz`, and applies final
protections without ever creating a writable-executable mapping. It then
rewrites target-owned auxiliary-vector entries and transfers directly to the
manifest entry point. V2 must not create a memfd, write a reconstructed ELF to a
file descriptor, or perform a second `execve`/`execveat`.

## Determinism and reversibility

The host encoder uses fixed properties and emits the same raw stream for the
same input. The manifest is canonical and ordered exactly as the original ELF
program-header table. For fixed input bytes, original mode, loader bytes, and
format versions, the complete v2 executable is byte-for-byte deterministic.
Host unpacking recovers the complete original file and mode exactly.
