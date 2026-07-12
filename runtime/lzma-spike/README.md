# LZMA1 feasibility spike

This isolated crate evaluates deterministic host encoding and the incremental
code size of a bounded freestanding LZMA1 decoder before either is admitted to
executable format v2. It is not linked into Packforge release artifacts.

The M2 go/no-go constraints are defined in `docs/plans/M2.md`: the complete
runtime must remain at or below 23,500 bytes, every projected corpus artifact
must remain within 105% of UPX 5.2.0 `--best`, and median size must be lower than
UPX. A candidate that misses either bound stays isolated here.

The rejected comparison encoders are optional dependencies so the default spike
build contains only the admitted pure-Rust candidate. Run them explicitly with
`--features comparison-encoders`. Compression ratio is measured before decoder
work because a deterministic but weak encoder cannot meet the artifact gate
regardless of decoder size.

## Results

`lzma-rs` 0.3.0 is rejected: its intentionally simple encoder produced payloads
of 466,229, 466,225, 280,928, and 907,220 bytes for the C, C++, Rust, and Go
fixtures. Those sizes cannot approach UPX.

The admitted encoder candidate is BSD-3-Clause `lzma-sdk-rs` 0.2301.1 at level 9
with its dictionary reduced to the exact input size. Its raw stream plus five
property bytes measured 278,754, 278,679, 181,423, and 586,576 bytes and each
stream round-tripped through both its reference decoder and Packforge's bounded
allocation-free runtime decoder.

Building the complete current runtime while retaining the new decoder increased
the reproducible loader from 10,888 to 14,776 bytes, an incremental 3,888 bytes.
That is 8,724 bytes below the 23,500-byte M2 go/no-go limit. The decoder remains
feature-retained only for this size measurement and is not selected by executable
format v1.

Run `scripts/ci-lzma-spike.sh` on Linux x86-64 to rebuild the exact four-language
corpus, encode every input twice, decode it through the runtime implementation,
rebuild the feature-retained loader, and compare projected v2 artifact sizes with
pinned UPX 5.2.0 `--best`. The projection includes the fixed container header,
executable trailer, and actual manifest size for each fixture; it remains a
feasibility result until executable v2 is implemented and benchmarked directly.
