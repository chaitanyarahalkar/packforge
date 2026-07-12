#!/usr/bin/env python3
"""Extract coarse v2 loader phase timings from timestamped strace evidence."""

from __future__ import annotations

import argparse
import json
import re
from decimal import Decimal
from pathlib import Path
from typing import NamedTuple


PHASES = ("payload_read", "payload_hash", "decompress", "map_segments", "transfer")
LINE = re.compile(
    r"^\s*(?P<pid>\d+)\s+(?P<start>\d+\.\d+)\s+"
    r"(?P<name>[a-z0-9_]+)\((?P<arguments>.*)\)\s+=.*"
    r"<(?P<duration>\d+\.\d+)>\s*$"
)
FIXED_MMAP = re.compile(
    r"^(?P<address>0x[0-9a-f]+),\s*(?P<length>\d+),.*MAP_FIXED_NOREPLACE"
)
MPROTECT = re.compile(r"^(?P<address>0x[0-9a-f]+),")
TRAILING_LENGTH = re.compile(r",\s*(?P<length>\d+),\s*\d+\s*$")


class TraceError(ValueError):
    """Raised when a trace cannot prove the expected loader sequence."""


class Event(NamedTuple):
    pid: int
    start_ns: int
    duration_ns: int
    name: str
    arguments: str

    @property
    def end_ns(self) -> int:
        return self.start_ns + self.duration_ns


def nanoseconds(value: str) -> int:
    """Convert a decimal second value to exact integer nanoseconds."""
    return int(Decimal(value) * 1_000_000_000)


def read_events(path: Path) -> list[Event]:
    """Read complete timestamped syscall records from one strace file."""
    events = []
    try:
        lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    except OSError as error:
        raise TraceError(f"could not read trace {path}: {error}") from error
    for line in lines:
        match = LINE.match(line)
        if match is None:
            continue
        events.append(
            Event(
                pid=int(match.group("pid")),
                start_ns=nanoseconds(match.group("start")),
                duration_ns=nanoseconds(match.group("duration")),
                name=match.group("name"),
                arguments=match.group("arguments"),
            )
        )
    if not events:
        raise TraceError(f"trace has no complete timestamped syscalls: {path}")
    return events


def difference(later: int, earlier: int, phase: str, path: Path) -> int:
    value = later - earlier
    if value < 0:
        raise TraceError(f"negative {phase} timing in trace: {path}")
    return value


def parse_trace(path: Path) -> dict[str, int]:
    """Extract one sample for every M2 phase from a loader execution trace."""
    events = read_events(path)
    initial_exec = next((event for event in events if event.name == "execve"), None)
    if initial_exec is None:
        raise TraceError(f"trace has no initial execve: {path}")
    loader = [event for event in events if event.pid == initial_exec.pid]

    fixed_mappings: list[tuple[Event, int, int]] = []
    for event in loader:
        if event.name != "mmap":
            continue
        match = FIXED_MMAP.match(event.arguments)
        if match is not None:
            fixed_mappings.append(
                (event, int(match.group("address"), 16), int(match.group("length")))
            )
    if not fixed_mappings:
        raise TraceError(f"trace has no fixed target mappings: {path}")
    first_fixed = fixed_mappings[0][0]

    staging_mappings = [
        event
        for event in loader
        if event.name == "mmap"
        and event.start_ns < first_fixed.start_ns
        and "MAP_ANONYMOUS" in event.arguments
        and "MAP_FIXED_NOREPLACE" not in event.arguments
    ]
    if not staging_mappings:
        raise TraceError(f"trace has an incomplete staging-map sequence: {path}")
    direct_output = "PROT_READ|PROT_WRITE" in first_fixed.arguments
    phase_boundary = first_fixed if direct_output else staging_mappings[-1]
    preads = [event for event in loader if event.name == "pread64" and event.start_ns < phase_boundary.start_ns]
    if len(preads) < 4:
        raise TraceError(f"trace has an incomplete v2 pread sequence: {path}")
    payload_read = preads[-1]

    target_mprotects = []
    for event in loader:
        if event.name != "mprotect":
            continue
        match = MPROTECT.match(event.arguments)
        if match is None:
            continue
        address = int(match.group("address"), 16)
        if any(start <= address < start + length for _, start, length in fixed_mappings):
            target_mprotects.append(event)
    if not target_mprotects:
        raise TraceError(f"trace has no target mprotect sequence: {path}")
    if direct_output:
        manifest_length_match = TRAILING_LENGTH.search(preads[-2].arguments)
        if manifest_length_match is None:
            raise TraceError(f"trace has no direct-output manifest length: {path}")
        manifest_length = int(manifest_length_match.group("length"))
        segment_count, remainder = divmod(manifest_length - 40, 48)
        if manifest_length < 40 or remainder != 0 or segment_count == 0:
            raise TraceError(f"trace has an invalid direct-output manifest length: {path}")
        if len(target_mprotects) < segment_count:
            raise TraceError(f"trace has an incomplete target protection sequence: {path}")
        first_target_mprotect = target_mprotects[0]
        last_target_mprotect = target_mprotects[segment_count - 1]
    else:
        first_target_mprotect = first_fixed
        last_target_mprotect = target_mprotects[-1]

    target_write = next(
        (
            event
            for event in events
            if event.name == "write"
            and event.start_ns >= last_target_mprotect.end_ns
            and event.arguments.startswith("1,")
        ),
        None,
    )
    if target_write is None:
        raise TraceError(f"trace has no target stdout write after transfer: {path}")

    if direct_output:
        payload_hash_end = first_fixed.start_ns
        decompress_start = first_fixed.end_ns
        decompress_end = first_target_mprotect.start_ns
        map_start = first_target_mprotect.start_ns
    else:
        original_mapping = staging_mappings[-1]
        payload_hash_end = original_mapping.start_ns
        decompress_start = original_mapping.end_ns
        decompress_end = first_fixed.start_ns
        map_start = first_fixed.start_ns

    return {
        "payload_read": payload_read.duration_ns,
        "payload_hash": difference(payload_hash_end, payload_read.end_ns, "payload_hash", path),
        "decompress": difference(decompress_end, decompress_start, "decompress", path),
        "map_segments": difference(last_target_mprotect.end_ns, map_start, "map_segments", path),
        "transfer": difference(target_write.start_ns, last_target_mprotect.end_ns, "transfer", path),
    }


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("traces", nargs="+", type=Path)
    parser.add_argument("--output", required=True, type=Path)
    arguments = parser.parse_args()

    samples = {phase: [] for phase in PHASES}
    for trace in arguments.traces:
        result = parse_trace(trace)
        for phase in PHASES:
            samples[phase].append(result[phase])
    arguments.output.parent.mkdir(parents=True, exist_ok=True)
    arguments.output.write_text(
        json.dumps(samples, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
