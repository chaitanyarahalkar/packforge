# Runtime stubs

Runtime stubs will live here once manifest and round-trip container work is stable.

Each target gets a separately built, versioned stub with a narrowly documented ABI.
Stub binaries are generated artifacts; their source, linker scripts, build command,
hashes, disassembly checks, and size budget must be reviewable in this repository.

No runtime code should be added before the manifest limits and malformed-container
tests are in place.

