#!/usr/bin/env python3
"""Validate M0 corpus metadata and construct/evaluate benchmark report v1."""

from __future__ import annotations

import argparse
import csv
import datetime as dt
import hashlib
import json
import os
import platform
import subprocess
import sys
from pathlib import Path
from typing import Any


ARTIFACT_KINDS = ("original", "packforge", "upx")
METRICS = ("warm_time_ns", "cold_time_ns", "peak_rss_kib")
SUMMARY_FIELDS = (
    "fixture",
    "kind",
    "bytes",
    "ratio_bp",
    "sha256",
    "behavior_matches_original",
    "reversible",
    "deterministic",
    "warm_median_ns",
    "cold_median_ns",
    "rss_median_kib",
)
RAW_FIELDS = ("fixture", "kind", "metric", "sample", "value")


class ContractError(ValueError):
    """Benchmark input violates the versioned contract."""


def load_json(path: Path) -> dict[str, Any]:
    with path.open(encoding="utf-8") as stream:
        value = json.load(stream)
    if not isinstance(value, dict):
        raise ContractError(f"{path} must contain a JSON object")
    return value


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def validate_corpus(workspace: Path, corpus_path: Path) -> dict[str, Any]:
    corpus = load_json(corpus_path)
    if corpus.get("schema_version") != 1:
        raise ContractError("corpus schema_version must be 1")
    if corpus.get("license") != "MIT":
        raise ContractError("M0 corpus must have the repository MIT license")
    fixtures = corpus.get("fixtures")
    if not isinstance(fixtures, list) or not fixtures:
        raise ContractError("corpus fixtures must be a nonempty array")

    identifiers: set[str] = set()
    for fixture in fixtures:
        if not isinstance(fixture, dict):
            raise ContractError("each corpus fixture must be an object")
        identifier = fixture.get("id")
        if not isinstance(identifier, str) or not identifier:
            raise ContractError("fixture id must be a nonempty string")
        if identifier in identifiers:
            raise ContractError(f"duplicate fixture id {identifier}")
        identifiers.add(identifier)

        source_value = fixture.get("source")
        if not isinstance(source_value, str):
            raise ContractError(f"fixture {identifier} source must be a string")
        source = Path(source_value)
        if source.is_absolute() or ".." in source.parts:
            raise ContractError(f"fixture {identifier} source must stay within the workspace")
        source_path = workspace / source
        if not source_path.is_file():
            raise ContractError(f"fixture {identifier} source does not exist: {source}")
        expected_digest = fixture.get("source_sha256")
        actual_digest = sha256(source_path)
        if expected_digest != actual_digest:
            raise ContractError(
                f"fixture {identifier} source digest mismatch: "
                f"expected {expected_digest}, got {actual_digest}"
            )
        for field in ("language", "target", "compiler", "arguments"):
            if field not in fixture:
                raise ContractError(f"fixture {identifier} is missing {field}")
    return corpus


def read_tsv(path: Path, fields: tuple[str, ...]) -> list[dict[str, str]]:
    with path.open(encoding="utf-8", newline="") as stream:
        reader = csv.DictReader(stream, delimiter="\t")
        if tuple(reader.fieldnames or ()) != fields:
            raise ContractError(
                f"{path} fields must be {fields}, got {tuple(reader.fieldnames or ())}"
            )
        return list(reader)


def parse_bool(value: str, field: str) -> bool:
    if value == "true":
        return True
    if value == "false":
        return False
    raise ContractError(f"{field} must be true or false, got {value!r}")


def parse_nonnegative(value: str, field: str) -> int:
    try:
        parsed = int(value)
    except ValueError as error:
        raise ContractError(f"{field} must be an integer, got {value!r}") from error
    if parsed < 0:
        raise ContractError(f"{field} must be nonnegative")
    return parsed


def integer_median(values: list[int]) -> int:
    if not values:
        return 0
    ordered = sorted(values)
    middle = len(ordered) // 2
    if len(ordered) % 2:
        return ordered[middle]
    return (ordered[middle - 1] + ordered[middle]) // 2


def first_line(command: list[str]) -> str:
    completed = subprocess.run(
        command,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    return completed.stdout.splitlines()[0].strip()


def git_commit(workspace: Path) -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=workspace,
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    commit = completed.stdout.strip()
    if len(commit) != 40 or any(character not in "0123456789abcdef" for character in commit):
        raise ContractError("git did not return a full lowercase commit digest")
    return commit


def os_name() -> str:
    release = Path("/etc/os-release")
    if release.is_file():
        for line in release.read_text(encoding="utf-8").splitlines():
            if line.startswith("PRETTY_NAME="):
                return line.partition("=")[2].strip().strip('"')
    return platform.platform()


def cpu_name() -> str:
    cpuinfo = Path("/proc/cpuinfo")
    if cpuinfo.is_file():
        for line in cpuinfo.read_text(encoding="utf-8").splitlines():
            if line.startswith("model name"):
                return line.partition(":")[2].strip()
    return platform.processor() or "unknown"


def calculate_gates(fixtures: list[dict[str, Any]]) -> dict[str, Any]:
    size_ratios: list[int] = []
    cold_ratios: list[int] = []
    correctness = True
    for fixture in fixtures:
        artifacts = {artifact["kind"]: artifact for artifact in fixture["artifacts"]}
        if set(artifacts) != set(ARTIFACT_KINDS):
            raise ContractError(f"fixture {fixture['id']} must contain exactly three artifact kinds")
        original = artifacts["original"]
        packforge = artifacts["packforge"]
        upx = artifacts["upx"]
        correctness = correctness and all(
            artifact["behavior_matches_original"] for artifact in artifacts.values()
        )
        correctness = correctness and packforge["reversible"] and packforge["deterministic"]
        correctness = correctness and original["reversible"] and original["deterministic"]
        size_ratios.append(packforge["bytes"] * 10_000 // upx["bytes"])
        packforge_cold = packforge["cold_time_ns"]["median"]
        upx_cold = upx["cold_time_ns"]["median"]
        if packforge_cold and upx_cold:
            cold_ratios.append(packforge_cold * 10_000 // upx_cold)

    size_ratio = integer_median(size_ratios)
    size_pass = size_ratio <= 10_500
    cold_ratio = integer_median(cold_ratios) if len(cold_ratios) == len(fixtures) else None
    cold_pass = None if cold_ratio is None else cold_ratio < 10_000 or size_ratio < 10_000
    return {
        "correctness_pass": correctness,
        "size_limit_basis_points": 10_500,
        "size_packforge_over_upx_basis_points": size_ratio,
        "size_pass": size_pass,
        "cold_packforge_over_upx_basis_points": cold_ratio,
        "cold_pass": cold_pass,
        "release_pass": correctness and size_pass and cold_pass is True,
    }


def build_report(arguments: argparse.Namespace) -> dict[str, Any]:
    workspace = arguments.workspace.resolve()
    corpus = validate_corpus(workspace, arguments.corpus)
    summary_rows = read_tsv(arguments.summary, SUMMARY_FIELDS)
    raw_rows = read_tsv(arguments.raw, RAW_FIELDS)

    summaries: dict[tuple[str, str], dict[str, str]] = {}
    for row in summary_rows:
        key = (row["fixture"], row["kind"])
        if key in summaries:
            raise ContractError(f"duplicate summary row for {key}")
        summaries[key] = row

    raw: dict[tuple[str, str, str], list[tuple[int, int]]] = {}
    for row in raw_rows:
        key = (row["fixture"], row["kind"], row["metric"])
        if row["metric"] not in METRICS:
            raise ContractError(f"unknown raw metric {row['metric']}")
        sample = parse_nonnegative(row["sample"], "sample")
        value = parse_nonnegative(row["value"], "value")
        raw.setdefault(key, []).append((sample, value))

    fixture_reports: list[dict[str, Any]] = []
    warm_iterations: int | None = None
    cold_iterations: int | None = None
    for fixture in corpus["fixtures"]:
        artifacts: list[dict[str, Any]] = []
        for kind in ARTIFACT_KINDS:
            key = (fixture["id"], kind)
            if key not in summaries:
                raise ContractError(f"missing summary row for {key}")
            row = summaries[key]
            metric_reports: dict[str, dict[str, Any]] = {}
            for metric, summary_field in (
                ("warm_time_ns", "warm_median_ns"),
                ("cold_time_ns", "cold_median_ns"),
                ("peak_rss_kib", "rss_median_kib"),
            ):
                samples = sorted(raw.get((fixture["id"], kind, metric), []))
                if [sample for sample, _ in samples] != list(range(len(samples))):
                    raise ContractError(f"samples for {key} {metric} must be contiguous from zero")
                values = [value for _, value in samples]
                reported_median = parse_nonnegative(row[summary_field], summary_field)
                if integer_median(values) != reported_median:
                    raise ContractError(f"median mismatch for {key} {metric}")
                metric_reports[metric] = {"median": reported_median, "values": values}
            current_warm = len(metric_reports["warm_time_ns"]["values"])
            current_cold = len(metric_reports["cold_time_ns"]["values"])
            if warm_iterations is None:
                warm_iterations = current_warm
                cold_iterations = current_cold
            if current_warm != warm_iterations or current_cold != cold_iterations:
                raise ContractError("all artifacts must contain equal warm and cold sample counts")
            if len(metric_reports["peak_rss_kib"]["values"]) != warm_iterations:
                raise ContractError("RSS sample count must equal warm iteration count")
            artifacts.append(
                {
                    "kind": kind,
                    "bytes": parse_nonnegative(row["bytes"], "bytes"),
                    "ratio_basis_points": parse_nonnegative(row["ratio_bp"], "ratio_bp"),
                    "sha256": row["sha256"],
                    "behavior_matches_original": parse_bool(
                        row["behavior_matches_original"], "behavior_matches_original"
                    ),
                    "reversible": parse_bool(row["reversible"], "reversible"),
                    "deterministic": parse_bool(row["deterministic"], "deterministic"),
                    **metric_reports,
                }
            )
        fixture_reports.append(
            {
                "id": fixture["id"],
                "source": fixture["source"],
                "source_sha256": fixture["source_sha256"],
                "artifacts": artifacts,
            }
        )

    if warm_iterations is None or cold_iterations is None:
        raise ContractError("benchmark report contains no fixture data")
    architecture = platform.machine()
    if architecture != "x86_64":
        raise ContractError(f"native report requires x86_64, got {architecture}")
    packforge_binary = workspace / "target/release/packforge"
    report = {
        "schema_version": 1,
        "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat().replace("+00:00", "Z"),
        "source_commit": git_commit(workspace),
        "source_run_url": arguments.source_run_url,
        "environment": {
            "os": os_name(),
            "kernel": platform.release(),
            "architecture": architecture,
            "cpu": cpu_name(),
            "runner": arguments.runner,
        },
        "tools": {
            "cc": first_line(["cc", "--version"]),
            "cpp": first_line(["c++", "--version"]),
            "rustc": first_line(["rustc", "--version"]),
            "go": first_line(["go", "version"]),
            "packforge": first_line([str(packforge_binary), "--version"]),
            "upx": "5.2.0",
        },
        "configuration": {
            "packforge_profile": "fast",
            "upx_mode": "--best",
            "warmup_iterations": 1,
            "warm_iterations": warm_iterations,
            "cold_iterations": cold_iterations,
            "cold_cache_reset": arguments.cold_cache_reset,
        },
        "fixtures": fixture_reports,
        "gates": calculate_gates(fixture_reports),
    }
    return report


def validate_report(report: dict[str, Any]) -> None:
    if report.get("schema_version") != 1:
        raise ContractError("report schema_version must be 1")
    fixtures = report.get("fixtures")
    if not isinstance(fixtures, list) or not fixtures:
        raise ContractError("report fixtures must be a nonempty array")
    expected = calculate_gates(fixtures)
    if report.get("gates") != expected:
        raise ContractError("embedded benchmark gates do not match artifact measurements")


def command_validate_corpus(arguments: argparse.Namespace) -> int:
    corpus = validate_corpus(arguments.workspace.resolve(), arguments.corpus)
    print(f"validated corpus v{corpus['schema_version']}: {len(corpus['fixtures'])} fixtures")
    return 0


def command_report(arguments: argparse.Namespace) -> int:
    report = build_report(arguments)
    arguments.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        f"wrote benchmark report v1: {len(report['fixtures'])} fixtures, "
        f"release_pass={str(report['gates']['release_pass']).lower()}"
    )
    return 0


def command_evaluate(arguments: argparse.Namespace) -> int:
    report = load_json(arguments.report)
    validate_report(report)
    gates = report["gates"]
    print(json.dumps(gates, indent=2))
    if arguments.enforce and not gates["release_pass"]:
        return 1
    return 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    subcommands = root.add_subparsers(dest="command", required=True)

    corpus = subcommands.add_parser("validate-corpus")
    corpus.add_argument("--workspace", type=Path, default=Path("."))
    corpus.add_argument("--corpus", type=Path, default=Path("benchmarks/corpus-v1.json"))
    corpus.set_defaults(function=command_validate_corpus)

    report = subcommands.add_parser("report")
    report.add_argument("--workspace", type=Path, default=Path("."))
    report.add_argument("--corpus", type=Path, default=Path("benchmarks/corpus-v1.json"))
    report.add_argument("--summary", type=Path, required=True)
    report.add_argument("--raw", type=Path, required=True)
    report.add_argument("--output", type=Path, required=True)
    report.add_argument("--source-run-url", default=os.environ.get("GITHUB_SERVER_URL", ""))
    report.add_argument("--runner", default=os.environ.get("RUNNER_NAME", "local"))
    report.add_argument(
        "--cold-cache-reset",
        choices=("none", "linux_drop_caches_3"),
        default="none",
    )
    report.set_defaults(function=command_report)

    evaluate = subcommands.add_parser("evaluate")
    evaluate.add_argument("report", type=Path)
    evaluate.add_argument("--enforce", action="store_true")
    evaluate.set_defaults(function=command_evaluate)
    return root


def main() -> int:
    arguments = parser().parse_args()
    try:
        return arguments.function(arguments)
    except (ContractError, OSError, subprocess.CalledProcessError) as error:
        print(f"benchmark contract error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
