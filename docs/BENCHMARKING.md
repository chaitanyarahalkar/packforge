# Executable benchmark method

Packforge pins UPX 5.2.0 as the current reference baseline for the M2 runtime
spike. The official amd64 Linux archive is downloaded from the UPX GitHub release
and accepted only when its SHA-256 is
`3db5d3294707439db97866feab8d75d800f028f48481a40547411824da4288a1`.

Run the warm benchmark on Linux x86-64:

```bash
mkdir -p benchmark-output
PACKFORGE_BENCHMARK_RAW=benchmark-output/raw-samples.tsv \
  bash scripts/benchmark-linux-executable.sh 21 0 \
  > benchmark-output/summary.tsv
```

The second numeric argument is the cold iteration count. Cold collection is
permitted only on a dedicated root-capable runner because every sample resets the
complete Linux filesystem page cache. The manual CI benchmark uses seven cold
samples:

```bash
PACKFORGE_BENCHMARK_RAW=benchmark-output/raw-samples.tsv \
PACKFORGE_DROP_CACHES=1 \
  bash scripts/benchmark-linux-executable.sh 21 7 \
  > benchmark-output/summary.tsv
```

The script builds deterministic static C, C++, Rust/musl, and pure-Go fixtures.
CI pins Rust 1.97.0 and Go 1.26.4; native compiler and kernel versions must be
recorded alongside any published result.
For each fixture it compares:

- the original executable;
- Packforge executable output using the currently supported `fast` runtime
  profile;
- UPX 5.2.0 using `--best`.

Every produced artifact must execute with identical stdout, stderr, and status
before measurement. Packforge output must also unpack byte-identically and repeat
packing must be deterministic. The collector validates the versioned fixture
source digests before compilation.

The tab-separated view includes exact artifact bytes, SHA-256, size ratio in basis
points, correctness facts, median warm and cold process duration, and median peak
RSS. Raw samples are written separately. The default is 21 measured warm
iterations after one warm-up; an odd iteration count from 3 through 101 may be
selected. Cold iterations are zero or an odd count from 3 through 31.

Construct and evaluate the versioned JSON report with:

```bash
python3 scripts/benchmark_contract.py metadata \
  --output benchmark-output/metadata.json
python3 scripts/benchmark_contract.py report \
  --summary benchmark-output/summary.tsv \
  --raw benchmark-output/raw-samples.tsv \
  --metadata benchmark-output/metadata.json \
  --output benchmark-output/report.json
python3 scripts/benchmark_contract.py evaluate benchmark-output/report.json
```

Metadata is collected exactly once and retained with the raw samples, making
subsequent report assembly byte-for-byte deterministic. The evaluator always
reports failed performance gates. Pass `--enforce` only for
a release gate; ordinary development and baseline runs must retain failing data.
The authoritative contract and exit evidence are in `plans/M0.md`.

## M2 performance and syscall contract

M2 retains the complete version 1 timing report and combines it with one
non-timed `strace` execution and one inspect report per Packforge fixture. Set
`PACKFORGE_RUNTIME_TRACES` when collecting:

```bash
PACKFORGE_BENCHMARK_RAW=benchmark-output/raw-samples.tsv \
PACKFORGE_RUNTIME_TRACES=benchmark-output/runtime-traces \
PACKFORGE_DROP_CACHES=1 \
  bash scripts/benchmark-linux-executable.sh 21 7 \
  > benchmark-output/summary.tsv
```

After producing `report.json`, build the strict M2 view:

```bash
python3 scripts/m2_performance_contract.py build \
  --benchmark-report benchmark-output/report.json \
  --trace-directory benchmark-output/runtime-traces \
  --loader runtime/artifacts/linux-x86_64/loader-v2 \
  --output benchmark-output/m2-report.json
python3 scripts/m2_performance_contract.py evaluate \
  benchmark-output/m2-report.json
```

The v2 evaluator requires the same report to pass correctness, reversibility,
determinism, the 23,500-byte loader limit, median size below UPX, every size within
105% of UPX, median cold time below UPX, median RSS within 110%, direct mapping,
zero `memfd_create`, and no secondary exec. Development builds may add
`FIXTURE.phases.json` files containing nonempty nanosecond sample arrays for
payload read/hash, decompression, segment mapping, and transfer. Missing phase
files are represented explicitly as `null`; aggregate process timing is never
silently presented as phase attribution.

Warm-start measurements are microbenchmarks. Cold measurements are valid only
when the report records `linux_drop_caches_3` on a dedicated runner. Results
produced under architecture translation are useful for regression comparison but
must not be published as native performance claims.
