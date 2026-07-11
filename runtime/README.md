# Runtime stubs

Each target has a separately built, versioned stub with a narrowly documented ABI.
The first implementation is the independent `no_std` Rust crate in
`linux-x86_64/`. Keeping it outside the host workspace isolates its audited raw
syscalls and pointer operations from the host crates, which continue to forbid
unsafe Rust.

The Linux loader is built for `x86_64-unknown-linux-musl`, but it is freestanding:
it has no libc, interpreter, or dynamically linked dependencies. Its release
binary must stay under the 32 KiB initial budget and is embedded into the host
packer as a versioned artifact. Linux CI rebuilds the source, verifies the exact
artifact digest, and inspects its ELF headers and symbols. The build removes the
non-loadable `.comment` section before hashing because LLD records a host-specific
source identifier there; loadable bytes remain identical across build hosts.

The wrapper ABI and validation order are specified in
`../docs/EXECUTABLE_FORMAT.md`. Runtime behavior and go/no-go gates remain in
`../docs/RUNTIME_SPIKE.md`.

`zstd-spike/` contains the reproducible `ruzstd` 0.8.3 size and compatibility
experiment. Its optimized decoder delta is 59,936 bytes, so the experiment is a
no-go for the 32 KiB M2 loader budget. It is intentionally isolated from the
production runtime.
