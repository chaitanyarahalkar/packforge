# Executable benchmark method

Packforge pins UPX 5.2.0 as the current reference baseline for the M2 runtime
spike. The official amd64 Linux archive is downloaded from the UPX GitHub release
and accepted only when its SHA-256 is
`3db5d3294707439db97866feab8d75d800f028f48481a40547411824da4288a1`.

Run the benchmark on Linux x86-64:

```bash
bash scripts/benchmark-linux-executable.sh 21
```

The script builds deterministic static C, C++, Rust/musl, and pure-Go fixtures.
CI pins Rust 1.97.0 and Go 1.26.4; native compiler and kernel versions must be
recorded alongside any published result.
For each fixture it compares:

- the original executable;
- Packforge executable output using the currently supported `fast` runtime
  profile;
- UPX 5.2.0 using `--best`.

Every produced artifact must execute with the expected output before measurement.
The tab-separated report includes exact artifact bytes, size ratio in basis
points, median warm process duration, and median peak RSS. The default is 21
measured iterations after one warm-up; an odd iteration count from 3 through 101
may be selected.

This is a warm-start microbenchmark. Cold-start results require a dedicated
runner that can reset filesystem caches between samples without affecting other
jobs. Results produced under architecture translation are useful for regression
comparison but must not be published as native performance claims.
