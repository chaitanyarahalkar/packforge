# M2 direct-runtime benchmark — Linux x86-64 — 2026-07-11

This directory preserves the native release-campaign evidence from
[GitHub Actions run 29177085582](https://github.com/chaitanyarahalkar/packforge/actions/runs/29177085582)
at commit `f5b5e9e004f924b529ebc6498819c7aeaf68590d`.

The comparison uses Packforge executable v2 with the `balanced` raw-LZMA1
profile and pinned UPX 5.2.0 `--best`. It records 21 warm samples and seven cold
samples per artifact; each cold sample follows Linux `drop_caches=3`.

## Result

- correctness, reversibility, determinism: pass
- direct W^X mapping, no memfd, no secondary exec: pass
- loader size: 17,360 bytes, below the 23,500-byte ceiling
- median Packforge/UPX size: 96.02%, pass
- every Packforge/UPX size: at most 105%, pass
- median Packforge/UPX cold time: 389.50%, fail
- median Packforge/UPX RSS: 102.02%, pass
- overall M2 release gate: **fail**

Packforge is smaller than UPX on the median fixture and on C, C++, and Go. The
Rust fixture is 1.59% larger. Packforge does not yet beat UPX overall because its
bounded LZMA1 decode and verification path is substantially slower at startup.

`summary.tsv` is the compact measurement table. `raw-samples.tsv` contains every
timing/RSS sample. `report.json` and `m2-report.json` are the validated aggregate
contracts; their corresponding gate files contain the exact verdicts.
`runtime-traces/` preserves the non-timed syscall and inspect evidence.
