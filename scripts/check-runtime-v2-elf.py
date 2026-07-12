#!/usr/bin/env python3
"""Validate the bounded ELF properties required by Packforge loader v2."""

from __future__ import annotations

import struct
import sys
from pathlib import Path

PT_LOAD = 1
PT_DYNAMIC = 2
PT_INTERP = 3
PT_GNU_STACK = 0x6474E551
PF_X = 1
PF_W = 2
DT_NULL = 0
DT_NEEDED = 1
RELOCATION_TAGS = {7, 8, 17, 18, 23}


def checked_range(data: bytes, offset: int, length: int, name: str) -> bytes:
    end = offset + length
    if offset < 0 or length < 0 or end > len(data):
        raise ValueError(f"{name} range is outside the file")
    return data[offset:end]


def validate(path: Path) -> None:
    data = path.read_bytes()
    header = checked_range(data, 0, 64, "ELF header")
    if header[:6] != b"\x7fELF\x02\x01":
        raise ValueError("loader is not little-endian ELF64")
    elf_type, machine = struct.unpack_from("<HH", header, 16)
    if elf_type != 3 or machine != 62:
        raise ValueError("loader must be x86-64 ET_DYN")
    phoff = struct.unpack_from("<Q", header, 32)[0]
    phentsize, phnum = struct.unpack_from("<HH", header, 54)
    if phentsize != 56 or phnum == 0:
        raise ValueError("loader has an invalid program-header table")

    load_count = 0
    for index in range(phnum):
        program = checked_range(data, phoff + index * phentsize, phentsize, "program header")
        kind, flags, offset, _, _, file_size, _, _ = struct.unpack("<IIQQQQQQ", program)
        if kind == PT_INTERP:
            raise ValueError("loader must not contain PT_INTERP")
        if kind == PT_LOAD:
            load_count += 1
            if flags & (PF_W | PF_X) == (PF_W | PF_X):
                raise ValueError("loader contains a writable-executable PT_LOAD")
        if kind == PT_GNU_STACK and flags & PF_X:
            raise ValueError("loader requests an executable stack")
        if kind == PT_DYNAMIC:
            dynamic = checked_range(data, offset, file_size, "dynamic table")
            for entry_offset in range(0, len(dynamic), 16):
                tag, value = struct.unpack_from("<QQ", dynamic, entry_offset)
                if tag == DT_NULL:
                    break
                if tag == DT_NEEDED:
                    raise ValueError("loader has a dynamic dependency")
                if tag in RELOCATION_TAGS and value != 0:
                    raise ValueError("loader requires runtime relocation")
    if load_count == 0:
        raise ValueError("loader has no PT_LOAD segments")


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {Path(sys.argv[0]).name} LOADER", file=sys.stderr)
        return 2
    try:
        validate(Path(sys.argv[1]))
    except (OSError, ValueError, struct.error) as error:
        print(f"invalid loader v2: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
