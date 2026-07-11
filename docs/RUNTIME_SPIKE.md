# Linux x86-64 runtime spike

This spike tests an experimental diskless compatibility launcher before Packforge
commits to an in-process ELF loader. It is not the final M2 native runtime tier and
must not be described as one.

## Proposed artifact

```text
static x86-64 loader stub | LZ4 payload | fixed authenticated footer
```

The host packer continues to accept only the validated static, non-PIE ELF64
x86-64 tier. The executable wrapper uses LZ4 because its bounded block decoder is
small enough for a freestanding stub. Zstandard remains a host-container codec
until decoder size is measured against real binaries.

## Runtime sequence

1. Open `/proc/self/exe` read-only and seek to the fixed footer.
2. Validate magic, version, reserved fields, checked ranges, and hard limits.
3. Read the compressed payload into a non-executable anonymous mapping.
4. Verify its digest before decompression.
5. Decompress into a separately bounded read/write mapping.
6. Verify the exact original length and digest.
7. Create an executable anonymous file with `memfd_create`.
8. Write the reconstructed ELF, set its executable mode, and apply available
   write/grow/shrink seals.
9. Replace the launcher with the original ELF using
   `execveat(fd, "", argv, envp, AT_EMPTY_PATH)`.
10. On every error, write one stable diagnostic to standard error and exit without
    a disk fallback.

The loader never maps writable memory as executable. The kernel performs normal
ELF loading from the sealed anonymous file.

## Linux API constraints

- `memfd_create` is Linux-specific and available since Linux 3.17. Its file lives
  in RAM and is released after all references are dropped.
- `execveat` is available since Linux 3.19, which sets the spike's minimum kernel.
- `AT_EMPTY_PATH` executes the file referred to by the descriptor. The descriptor
  may use close-on-exec for the supported ELF-only tier; the documented interpreter
  script failure does not apply because scripts are rejected by the host parser.
- Newer kernels can require `MFD_EXEC` under the `vm.memfd_noexec` policy. The
  launcher should first request `MFD_EXEC | MFD_CLOEXEC | MFD_ALLOW_SEALING`, then
  retry without `MFD_EXEC` only when an older kernel rejects that unknown flag.
- The first spike requires `/proc/self/exe`. If procfs is unavailable, it fails
  closed. Locating an embedded mapped payload without procfs is a separate native
  loader design, not an implicit disk-extraction fallback.

## Observable compatibility differences

`execveat` preserves the process ID, argument vector, environment, working
directory, resource limits, and non-close-on-exec descriptors through the normal
exec transition. It does not preserve the executable pathname identity:
`/proc/self/exe` after launch refers to the anonymous file. Programs that reopen or
compare their own distribution path therefore require an explicit compatibility
classification.

Seccomp profiles can also deny `memfd_create` or `execveat`. Packforge will report
this as an unsupported runtime environment rather than extracting to `/tmp`.

## Go/no-go gates

- Stub is freestanding, reproducible, reviewable, and below a 32 KiB initial size
  budget before the payload/footer.
- No libc, dynamic loader, filesystem writes, network access, RWX memory, or
  environment-controlled decoder settings.
- Corrupt footer, oversized lengths, integer overflow, truncated reads, invalid
  LZ4 streams, and digest mismatches all fail before `execveat`.
- Original and wrapped fixtures match exit status, stdout, stderr, arguments,
  environment, working directory, signals, and inherited descriptor behavior.
- Corpus covers static C, C++, Rust, Go, musl, and glibc executables on pinned
  Linux 3.19-era, current LTS, and current kernels where practical.
- Startup RSS/time and total artifact size are compared with the original and UPX.
- `/proc/self/exe`-dependent programs are detected or documented as outside the
  compatibility tier.

If these gates fail, Packforge does not promote the launcher as M2. The project
then proceeds directly to the format-aware in-process loader spike described in
the architecture plan.
