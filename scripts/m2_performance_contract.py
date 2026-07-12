#!/usr/bin/env python3
"""Build and validate the M2 native performance report from raw v1 evidence."""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path
from typing import Any

import benchmark_contract


LOADER_SIZE_LIMIT = 23_500
SYSCALLS = (
    "execve",
    "execveat",
    "memfd_create",
    "mmap",
    "mprotect",
    "openat",
    "pread64",
    "write",
)
PHASES = ("payload_read", "payload_hash", "decompress", "map_segments", "transfer")


class ContractError(ValueError):
    """Raised when M2 evidence is missing, inconsistent, or malformed."""


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ContractError(f"could not read JSON {path}: {error}") from error
    if not isinstance(value, dict):
        raise ContractError(f"JSON root must be an object: {path}")
    return value


def ratio_basis_points(numerator: int, denominator: int) -> int:
    if numerator <= 0 or denominator <= 0:
        raise ContractError("performance ratios require positive measurements")
    return numerator * 10_000 // denominator


def parse_trace(path: Path) -> dict[str, int]:
    counts = {name: 0 for name in SYSCALLS}
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError as error:
        raise ContractError(f"could not read syscall trace {path}: {error}") from error
    pattern = re.compile(r"(?:^|\s)(" + "|".join(SYSCALLS) + r")\(")
    for line in lines:
        match = pattern.search(line)
        if match is not None:
            counts[match.group(1)] += 1
    if not lines or counts["execve"] == 0:
        raise ContractError(f"trace does not contain the initial execve: {path}")
    return counts


def read_phases(path: Path) -> dict[str, list[int]] | None:
    if not path.exists():
        return None
    value = load_json(path)
    if set(value) != set(PHASES):
        raise ContractError(f"phase file has missing or unknown fields: {path}")
    for phase, samples in value.items():
        if not isinstance(samples, list) or not samples:
            raise ContractError(f"phase {phase} must contain samples: {path}")
        if any(not isinstance(sample, int) or sample < 0 for sample in samples):
            raise ContractError(f"phase {phase} contains an invalid sample: {path}")
    return value


def artifact_by_kind(fixture: dict[str, Any], kind: str) -> dict[str, Any]:
    matches = [artifact for artifact in fixture["artifacts"] if artifact["kind"] == kind]
    if len(matches) != 1:
        raise ContractError(f"fixture {fixture.get('id')} must contain one {kind} artifact")
    return matches[0]


def summarize_artifact(artifact: dict[str, Any]) -> dict[str, int]:
    return {
        "bytes": artifact["bytes"],
        "warm_time_ns": artifact["warm_time_ns"]["median"],
        "cold_time_ns": artifact["cold_time_ns"]["median"],
        "peak_rss_kib": artifact["peak_rss_kib"]["median"],
    }


def inspect_payload(inspect_report: dict[str, Any], fixture_id: str) -> tuple[str, int, int | None]:
    if inspect_report.get("artifact_kind") != "executable":
        raise ContractError(f"fixture {fixture_id} inspect report is not an executable")
    container = inspect_report.get("container")
    if isinstance(container, dict):
        codec = container.get("codec")
        payload_size = container.get("payload_size")
    elif inspect_report.get("executable_version") == 2:
        codec = "lzma1"
        payload_size = inspect_report.get("payload_size")
    else:
        raise ContractError(f"fixture {fixture_id} inspect report has no payload metadata")
    if codec not in {"lz4", "lzma1"} or not isinstance(payload_size, int) or payload_size <= 0:
        raise ContractError(f"fixture {fixture_id} inspect report has invalid codec metadata")
    decoder_memory = inspect_report.get("decoder_memory_bytes")
    if decoder_memory is not None and (
        not isinstance(decoder_memory, int) or decoder_memory <= 0
    ):
        raise ContractError(f"fixture {fixture_id} has invalid decoder memory metadata")
    return codec, payload_size, decoder_memory


def loader_metadata(path: Path, codec: str, decoder_memory: int, direct_mapping: bool) -> dict[str, Any]:
    try:
        data = path.read_bytes()
    except OSError as error:
        raise ContractError(f"could not read runtime loader {path}: {error}") from error
    if len(data) < 18 or data[:4] != b"\x7fELF" or data[4:6] != b"\x02\x01":
        raise ContractError("runtime loader must be little-endian ELF64")
    elf_type = int.from_bytes(data[16:18], "little")
    if elf_type not in {2, 3}:
        raise ContractError(f"runtime loader has unsupported ELF type {elf_type}")
    return {
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
        "elf_type": {2: "et_exec", 3: "et_dyn"}[elf_type],
        "runtime_codec": codec,
        "max_decoder_memory_bytes": decoder_memory,
        "direct_mapping": direct_mapping,
    }


def calculate_gates(fixtures: list[dict[str, Any]], loader: dict[str, Any]) -> dict[str, Any]:
    size_ratios = [fixture["size_packforge_over_upx_basis_points"] for fixture in fixtures]
    cold_ratios = [fixture["cold_packforge_over_upx_basis_points"] for fixture in fixtures]
    rss_ratios = [fixture["rss_packforge_over_upx_basis_points"] for fixture in fixtures]
    correctness = all(fixture["behavior_matches_original"] for fixture in fixtures)
    reversibility = all(fixture["reversible"] for fixture in fixtures)
    determinism = all(fixture["deterministic"] for fixture in fixtures)
    loader_size_pass = loader["bytes"] <= LOADER_SIZE_LIMIT
    median_size = benchmark_contract.integer_median(size_ratios)
    all_size = all(ratio <= 10_500 for ratio in size_ratios)
    size_win = median_size < 10_000 and all_size
    median_cold = benchmark_contract.integer_median(cold_ratios)
    cold_win = median_cold < 10_000
    median_rss = benchmark_contract.integer_median(rss_ratios)
    rss_pass = median_rss <= 11_000
    direct_mapping = loader["direct_mapping"]
    no_memfd = all(fixture["syscalls"]["memfd_create"] == 0 for fixture in fixtures)
    no_secondary_exec = all(
        fixture["syscalls"]["execveat"] == 0 and fixture["syscalls"]["execve"] <= 1
        for fixture in fixtures
    )
    release = all(
        (
            correctness,
            reversibility,
            determinism,
            loader_size_pass,
            size_win,
            cold_win,
            rss_pass,
            direct_mapping,
            no_memfd,
            no_secondary_exec,
        )
    )
    return {
        "correctness_pass": correctness,
        "reversibility_pass": reversibility,
        "determinism_pass": determinism,
        "loader_size_limit_bytes": LOADER_SIZE_LIMIT,
        "loader_size_pass": loader_size_pass,
        "median_size_packforge_over_upx_basis_points": median_size,
        "all_size_within_105_percent": all_size,
        "size_win_pass": size_win,
        "median_cold_packforge_over_upx_basis_points": median_cold,
        "cold_win_pass": cold_win,
        "median_rss_packforge_over_upx_basis_points": median_rss,
        "rss_pass": rss_pass,
        "direct_mapping_pass": direct_mapping,
        "no_memfd_pass": no_memfd,
        "no_secondary_exec_pass": no_secondary_exec,
        "release_pass": release,
    }


def build_report(arguments: argparse.Namespace) -> dict[str, Any]:
    baseline = load_json(arguments.benchmark_report)
    try:
        benchmark_contract.validate_report(baseline)
    except benchmark_contract.ContractError as error:
        raise ContractError(f"invalid benchmark report v1: {error}") from error
    if baseline["configuration"]["cold_iterations"] == 0:
        raise ContractError("M2 reports require native cold-start samples")

    fixtures = []
    codecs: set[str] = set()
    maximum_decoder_memory = 0
    for baseline_fixture in baseline["fixtures"]:
        fixture_id = baseline_fixture["id"]
        original = artifact_by_kind(baseline_fixture, "original")
        packforge = artifact_by_kind(baseline_fixture, "packforge")
        upx = artifact_by_kind(baseline_fixture, "upx")
        inspect_report = load_json(arguments.trace_directory / f"{fixture_id}.inspect.json")
        codec, payload_bytes, decoder_memory = inspect_payload(inspect_report, fixture_id)
        codecs.add(codec)
        if decoder_memory is None:
            decoder_memory = original["bytes"] + payload_bytes
        maximum_decoder_memory = max(maximum_decoder_memory, decoder_memory)
        syscalls = parse_trace(arguments.trace_directory / f"{fixture_id}.strace")
        packforge_summary = summarize_artifact(packforge)
        upx_summary = summarize_artifact(upx)
        fixtures.append(
            {
                "id": fixture_id,
                "packforge": packforge_summary,
                "upx": upx_summary,
                "payload_bytes": payload_bytes,
                "decoder_memory_bytes": decoder_memory,
                "size_packforge_over_upx_basis_points": ratio_basis_points(
                    packforge_summary["bytes"], upx_summary["bytes"]
                ),
                "cold_packforge_over_upx_basis_points": ratio_basis_points(
                    packforge_summary["cold_time_ns"], upx_summary["cold_time_ns"]
                ),
                "rss_packforge_over_upx_basis_points": ratio_basis_points(
                    packforge_summary["peak_rss_kib"], upx_summary["peak_rss_kib"]
                ),
                "behavior_matches_original": packforge["behavior_matches_original"],
                "reversible": packforge["reversible"],
                "deterministic": packforge["deterministic"],
                "syscalls": syscalls,
                "phase_timings_ns": read_phases(
                    arguments.trace_directory / f"{fixture_id}.phases.json"
                ),
            }
        )
    if len(codecs) != 1:
        raise ContractError("all M2 fixtures must use one runtime codec in a report")
    loader = loader_metadata(
        arguments.loader,
        codecs.pop(),
        maximum_decoder_memory,
        arguments.direct_mapping,
    )
    configuration = baseline["configuration"]
    report = {
        "schema_version": 2,
        "generated_at_utc": baseline["generated_at_utc"],
        "source_commit": baseline["source_commit"],
        "source_run_url": baseline["source_run_url"],
        "environment": baseline["environment"],
        "tools": baseline["tools"],
        "configuration": {
            "packforge_profile": configuration["packforge_profile"],
            "upx_mode": configuration["upx_mode"],
            "warm_iterations": configuration["warm_iterations"],
            "cold_iterations": configuration["cold_iterations"],
            "cold_cache_reset": configuration["cold_cache_reset"],
        },
        "loader": loader,
        "fixtures": fixtures,
        "gates": calculate_gates(fixtures, loader),
    }
    validate_report(report)
    return report


def validate_report(report: dict[str, Any]) -> None:
    required = {
        "schema_version",
        "generated_at_utc",
        "source_commit",
        "source_run_url",
        "environment",
        "tools",
        "configuration",
        "loader",
        "fixtures",
        "gates",
    }
    if set(report) != required or report.get("schema_version") != 2:
        raise ContractError("M2 report has missing/unknown fields or wrong schema version")
    if not re.fullmatch(r"[0-9a-f]{40}", report.get("source_commit", "")):
        raise ContractError("M2 report source commit must be a full lowercase Git digest")
    configuration = report.get("configuration")
    expected_configuration = {
        "packforge_profile",
        "upx_mode",
        "warm_iterations",
        "cold_iterations",
        "cold_cache_reset",
    }
    if not isinstance(configuration, dict) or set(configuration) != expected_configuration:
        raise ContractError("M2 report configuration is malformed")
    if configuration["packforge_profile"] not in {"fast", "balanced", "small", "auto"}:
        raise ContractError("M2 report has an unknown Packforge profile")
    if configuration["upx_mode"] != "--best" or configuration["cold_cache_reset"] != "linux_drop_caches_3":
        raise ContractError("M2 report must use pinned UPX and cold-cache settings")
    if not isinstance(report.get("tools"), dict) or report["tools"].get("upx") != "5.2.0":
        raise ContractError("M2 report must use pinned UPX 5.2.0")
    fixtures = report.get("fixtures")
    loader = report.get("loader")
    if not isinstance(fixtures, list) or not fixtures or not isinstance(loader, dict):
        raise ContractError("M2 report must contain loader and fixture evidence")
    expected_loader = {
        "bytes",
        "sha256",
        "elf_type",
        "runtime_codec",
        "max_decoder_memory_bytes",
        "direct_mapping",
    }
    if set(loader) != expected_loader:
        raise ContractError("M2 loader evidence has missing or unknown fields")
    if not isinstance(loader["bytes"], int) or loader["bytes"] <= 0:
        raise ContractError("M2 loader size is invalid")
    if not re.fullmatch(r"[0-9a-f]{64}", loader.get("sha256", "")):
        raise ContractError("M2 loader digest is invalid")
    expected_fixture = {
        "id",
        "packforge",
        "upx",
        "payload_bytes",
        "decoder_memory_bytes",
        "size_packforge_over_upx_basis_points",
        "cold_packforge_over_upx_basis_points",
        "rss_packforge_over_upx_basis_points",
        "behavior_matches_original",
        "reversible",
        "deterministic",
        "syscalls",
        "phase_timings_ns",
    }
    expected_artifact = {"bytes", "warm_time_ns", "cold_time_ns", "peak_rss_kib"}
    for fixture in fixtures:
        if not isinstance(fixture, dict) or set(fixture) != expected_fixture:
            raise ContractError("M2 fixture evidence has missing or unknown fields")
        for artifact_name in ("packforge", "upx"):
            artifact = fixture[artifact_name]
            if not isinstance(artifact, dict) or set(artifact) != expected_artifact:
                raise ContractError(f"M2 fixture {fixture['id']} has invalid {artifact_name} evidence")
            if any(not isinstance(value, int) or value <= 0 for value in artifact.values()):
                raise ContractError(f"M2 fixture {fixture['id']} has nonpositive measurements")
        if not isinstance(fixture["syscalls"], dict) or set(fixture["syscalls"]) != set(SYSCALLS):
            raise ContractError(f"M2 fixture {fixture['id']} has invalid syscall evidence")
        if any(
            not isinstance(value, int) or value < 0 for value in fixture["syscalls"].values()
        ):
            raise ContractError(f"M2 fixture {fixture['id']} has invalid syscall count")
        phases = fixture["phase_timings_ns"]
        if phases is not None:
            if not isinstance(phases, dict) or set(phases) != set(PHASES):
                raise ContractError(f"M2 fixture {fixture['id']} has invalid phase evidence")
            if any(
                not isinstance(samples, list)
                or not samples
                or any(not isinstance(sample, int) or sample < 0 for sample in samples)
                for samples in phases.values()
            ):
                raise ContractError(f"M2 fixture {fixture['id']} has invalid phase samples")
    identifiers = [fixture.get("id") for fixture in fixtures if isinstance(fixture, dict)]
    if len(identifiers) != len(fixtures) or len(set(identifiers)) != len(identifiers):
        raise ContractError("M2 fixture identifiers must be unique")
    if report.get("gates") != calculate_gates(fixtures, loader):
        raise ContractError("M2 embedded gates do not match measurements")


def command_build(arguments: argparse.Namespace) -> int:
    report = build_report(arguments)
    arguments.output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(
        f"wrote M2 performance report: {len(report['fixtures'])} fixtures, "
        f"release_pass={str(report['gates']['release_pass']).lower()}"
    )
    return 0


def command_evaluate(arguments: argparse.Namespace) -> int:
    report = load_json(arguments.report)
    validate_report(report)
    print(json.dumps(report["gates"], indent=2))
    if arguments.enforce and not report["gates"]["release_pass"]:
        return 1
    return 0


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="command", required=True)
    build = commands.add_parser("build", help="combine v1 timing and runtime trace evidence")
    build.add_argument("--benchmark-report", type=Path, required=True)
    build.add_argument("--trace-directory", type=Path, required=True)
    build.add_argument("--loader", type=Path, required=True)
    build.add_argument("--output", type=Path, required=True)
    build.add_argument("--direct-mapping", action="store_true")
    build.set_defaults(handler=command_build)
    evaluate = commands.add_parser("evaluate", help="validate and print embedded M2 gates")
    evaluate.add_argument("report", type=Path)
    evaluate.add_argument("--enforce", action="store_true")
    evaluate.set_defaults(handler=command_evaluate)
    return root


def main() -> int:
    arguments = parser().parse_args()
    try:
        return arguments.handler(arguments)
    except ContractError as error:
        print(f"error: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
