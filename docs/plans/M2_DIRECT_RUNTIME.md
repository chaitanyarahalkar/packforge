# M2 direct runtime implementation plan

**Status: implemented and compatibility-verified; M2 performance gate failed.**
The direct runtime passes the two-runner ASLR matrix, syscall traces, fuzzing, and
the four-language/behavior corpus. The 2026-07-11 release campaign passes size and
RSS gates but fails cold startup at 389.50% of UPX, so M2 remains open.

## Runtime artifact contract

- Build a separate `loader-v2` as static, relocation-free `ET_DYN` so ASLR places
  the loader away from the fixed-address `ET_EXEC` target.
- Require no `PT_INTERP`, dynamic dependency, runtime relocation, executable
  stack, or writable-executable `PT_LOAD` segment.
- Preserve the frozen `loader-v1` bytes and v1 host/runtime behavior.
- Retain the admitted shared LZMA1 decoder and keep the complete normalized v2
  loader at or below 23,500 bytes.

## Implementation sequence

1. Add a reproducible v2 linker/build check that proves `ET_DYN`, no relocations,
   no interpreter/dependencies, W^X program headers, and deterministic bytes.
2. Parse and authenticate the v2 trailer, 192-byte image header, manifest v0,
   codec properties, and every range before allocating target memory.
3. Decompress the complete original into one exact anonymous staging mapping,
   verify its digest, and re-parse the ELF header/program-header facts needed for
   auxiliary-vector repair.
4. Preflight every target mapping before changing process state: checked page
   rounding, no W+X flags, no declared or rounded-range overlap, no collision
   with existing mappings, and entry point inside an executable segment.
5. Reserve each target range with `MAP_FIXED_NOREPLACE`, copy initialized bytes,
   zero BSS, then apply final `mprotect` permissions. No mapping is executable
   while writable.
6. Rewrite target-owned auxiliary-vector entries (`AT_PHDR`, `AT_PHENT`,
   `AT_PHNUM`, `AT_ENTRY`, and `AT_BASE`) in the original kernel stack while
   preserving arguments, environment, random bytes, kernel-owned entries,
   inherited descriptors, signal state, and the initial `rdx` value.
7. Close only the runtime's own descriptor and jump directly to the original
   entry point with the original stack pointer. V2 has no memfd, reconstructed
   file write, or second `execve`/`execveat` path.

## Progressive gates

1. A synthetic one-segment target reaches an assembly sentinel with the expected
   stack and auxiliary-vector values.
2. Mapping collisions, overlaps, W+X segments, malformed manifests, impossible
   program-header locations, and corrupt LZMA streams fail before control
   transfer.
3. The C fixture passes, followed by C++, Rust/musl, and Go; each matches output,
   exit status, file effects, cwd, inherited descriptors, signals, auxv, and
   `/proc/self/exe` expectations defined for the wrapper model.
4. Native syscall traces contain no post-start `memfd_create`, full-original
   write, `execve`, or `execveat`.
5. The two-kernel ASLR matrix, fuzz campaigns, deterministic cross-host packing,
   dependency audits, and final UPX size/cold-start/RSS gates all pass before v2
   is selected by the CLI or M2 is marked complete.

Any failure before the final jump must use a stable bounded diagnostic and exit
without attempting a fallback launch mechanism. A size win alone does not admit
the runtime.
