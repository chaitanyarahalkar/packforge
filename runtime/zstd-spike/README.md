# Zstandard runtime decoder spike

This isolated crate measures the lower-bound code-size cost of decoding
Packforge's balanced/small Zstandard payloads with `ruzstd` 0.8.3. It is not
linked into the production loader.

Build the two comparable static binaries with the pinned toolchain:

```sh
cargo build --release --bins
wc -c \
  target/x86_64-unknown-linux-musl/release/io-baseline \
  target/x86_64-unknown-linux-musl/release/ruzstd-decoder
```

The baseline performs the same whole-file input/output work but does not decode.
On the pinned Linux x86-64 musl target, the stripped static binaries measure:

| Binary | Bytes |
| --- | ---: |
| `io-baseline` | 377,472 |
| `ruzstd-decoder` | 437,408 |
| Decoder delta | 59,936 |

The decoder successfully reconstructs the bytes emitted by Packforge's
balanced profile. However, the 59,936-byte lower-bound delta already exceeds
the production loader's complete 32 KiB budget. A production integration would
also need an allocator, bounded allocation policy, Packforge parsing and
integrity validation, and the launch path.

`ruzstd` supports `no_std` when default features are disabled, but it still
requires `alloc`. Its decoder also permits frame windows up to its own 100 MiB
limit, which is wider than the production runtime should accept without an
explicit Packforge bound. For M2, this spike is therefore a measured no-go:
executable output remains LZ4-only, while Zstandard remains available for
non-executable recovery containers.
