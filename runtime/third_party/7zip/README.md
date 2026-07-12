# Pinned 7-Zip LZMA decoder assembly

`LzmaDecOpt.asm` is copied byte-for-byte from the public-domain 7-Zip source at
commit `f9d78aff31a5f2521ae7ddbdc97c4a8855808959` (7-Zip 26.02), path
`Asm/x86/LzmaDecOpt.asm`. Its SHA-256 digest is
`bddfb31a59c49c8f25f75d19e7330437d2ca3ba81d9655fa427d7585521a3859`.
The source declares itself public domain.

`LzmaDecOpt.o.b64` is the base64 encoding of the Linux x86-64 ELF object built
from that source with Asmc commit
`4b669147521b277b9e050922e7c97cb8aa608f45` using:

```sh
asmc64 -elf64 -DABI_LINUX -Fo<output-directory>/ LzmaDecOpt.asm
```

The decoded object is 5,477 bytes and has SHA-256 digest
`3441d63c9e32ed3c89ecc2a79ec1f72c29924ede24b385d1d1d6c32e501962c8`.
Linux CI rebuilds the object from both pinned upstream commits and byte-compares
it before runtime artifacts are admitted. The checked-in base64 form allows the
same verified object to be linked reproducibly on non-x86 and non-Linux hosts.
