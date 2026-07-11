# Packforge segment manifest v0

Manifest v0 is the bounded binary segment description established by M0 for
future format-aware runtimes. It is not embedded in container v1; later container
versions may carry these exact semantics without redefining version 0 fields.

All integers are little-endian. A manifest contains a fixed 40-byte header and
one to 128 fixed 48-byte segment records. The maximum encoded size is 6,184
bytes.

## Header

| Offset | Size | Field | Value or meaning |
| ---: | ---: | --- | --- |
| 0 | 8 | magic | ASCII `PFGMAN00` |
| 8 | 2 | version | zero |
| 10 | 2 | header length | 40 |
| 12 | 2 | segment record length | 48 |
| 14 | 2 | segment count | 1 through 128 |
| 16 | 8 | reserved | zero |
| 24 | 8 | original file size | 1 byte through 1 GiB |
| 32 | 8 | entry point | original virtual entry address |

## Segment record

| Offset | Size | Field | Meaning |
| ---: | ---: | --- | --- |
| 0 | 8 | file offset | initialized bytes in the original file |
| 8 | 8 | file size | initialized byte count |
| 16 | 8 | virtual address | target virtual address |
| 24 | 8 | memory size | initialized plus zero-filled bytes |
| 32 | 8 | alignment | zero, one, or a power of two |
| 40 | 4 | flags | low bits: execute=1, write=2, read=4 |
| 44 | 4 | reserved | zero |

## Validation

Readers reject unknown framing, versions, flags, reserved bytes, zero segment
counts, counts above 128, trailing bytes, checked-arithmetic overflow, file ranges
outside the original, file sizes larger than memory sizes, empty memory ranges,
virtual-range overflow, invalid alignment, incongruent file/virtual alignment,
and total described memory above 1 GiB.

Encoding a valid value is deterministic. Decoding performs the same semantic
validation as encoding, so successfully decoded bytes always re-encode to the
same canonical representation.
