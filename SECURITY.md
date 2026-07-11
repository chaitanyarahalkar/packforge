# Security policy

## Scope and intent

Packforge is intended to compress trusted, first-party executable programs for
distribution. It is not intended to conceal program behavior or bypass security
products.

The project will not add encryption, anti-debugging, anti-analysis, polymorphic
stubs, signature spoofing, or security-product evasion features.

## Engineering requirements

- Input parsing must use checked arithmetic and bounded allocations.
- Corrupt or unsupported inputs must fail closed before output replacement.
- Decompression limits must be derived from authenticated manifest metadata and
  independently capped by the runtime.
- Packed payload integrity must be verified before control is transferred.
- Runtime memory must follow W^X: writable during reconstruction, then executable
  only after permissions are finalized. RWX mappings are prohibited.
- Packing must never overwrite the input unless an explicit future in-place flag
  is provided, and that path must use atomic replacement.
- Inspecting, verifying, or unpacking an artifact must never execute its payload.
- Fuzzing and malformed-input regression tests are release gates.

## Signing

Packing changes executable bytes and invalidates an existing platform signature.
The documented workflow will therefore be build, pack, verify, sign, and then
notarize or distribute. Packforge will not handle private signing keys directly in
its initial releases.

## Reporting

Until a private reporting channel is configured, do not open public issues for a
suspected vulnerability. Contact the repository owner privately through GitHub.

