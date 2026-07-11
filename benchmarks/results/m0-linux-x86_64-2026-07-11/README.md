# M0 native baseline — 2026-07-11

This directory preserves the first report generated under benchmark contract v1.

- Source commit: `f5de1314c3831bd92f6f18aec9d718f279ec1139`
- Source workflow: <https://github.com/chaitanyarahalkar/packforge/actions/runs/29172003145>
- Runner: GitHub-hosted Ubuntu 24.04.4 LTS, Linux 6.17, native x86-64
- CPU: AMD EPYC 9V74
- Samples: 21 warm executions after one warm-up; seven cold executions
- Cold reset: `sync` plus Linux `drop_caches=3` before every sample

`metadata.json` freezes the environment facts used during deterministic report
assembly. `report.json` is the versioned machine-readable artifact.
`raw-samples.tsv` retains all 588 measurements, `summary.tsv` is the
human-readable projection, and `gates.json` is the deterministic evaluator
output.

The correctness gate passes. The performance release gate intentionally fails:
the median Packforge artifact is 156.78% of the corresponding UPX artifact and
the median Packforge cold-start time is 140.42% of UPX. These failures are the M2
optimization baseline and must not be edited away or reframed as a win.
