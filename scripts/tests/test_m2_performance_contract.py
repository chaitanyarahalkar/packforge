import copy
import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


SCRIPTS = Path(__file__).parents[1]
sys.path.insert(0, str(SCRIPTS))
SCRIPT = SCRIPTS / "m2_performance_contract.py"
SPEC = importlib.util.spec_from_file_location("m2_performance_contract", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
m2 = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(m2)


class M2PerformanceContractTests(unittest.TestCase):
    def setUp(self):
        self.workspace = Path(__file__).parents[2]
        self.baseline = self.workspace / (
            "benchmarks/results/m0-linux-x86_64-2026-07-11/report.json"
        )
        self.loader = self.workspace / "runtime/artifacts/linux-x86_64/loader-v1"

    def build_baseline(self, directory: Path):
        report = json.loads(self.baseline.read_text(encoding="utf-8"))
        trace_directory = directory / "traces"
        trace_directory.mkdir()
        for fixture in report["fixtures"]:
            identifier = fixture["id"]
            original = next(
                artifact for artifact in fixture["artifacts"] if artifact["kind"] == "original"
            )
            packforge = next(
                artifact for artifact in fixture["artifacts"] if artifact["kind"] == "packforge"
            )
            payload_size = packforge["bytes"] - self.loader.stat().st_size - 320
            inspect = {
                "artifact_kind": "executable",
                "container": {"codec": "lz4", "payload_size": payload_size},
            }
            (trace_directory / f"{identifier}.inspect.json").write_text(
                json.dumps(inspect), encoding="utf-8"
            )
            trace = """execve(\"packed\", [\"packed\"], []) = 0
openat(AT_FDCWD, \"/proc/self/exe\", O_RDONLY) = 3
pread64(3, \"...\", 128, 1) = 128
mmap(NULL, 4096, PROT_READ|PROT_WRITE, MAP_PRIVATE|MAP_ANONYMOUS, -1, 0) = 0
memfd_create(\"packforge\", 3) = 4
write(4, \"...\", 3) = 3
execveat(4, \"\", [\"packed\"], [], AT_EMPTY_PATH) = 0
"""
            (trace_directory / f"{identifier}.strace").write_text(trace, encoding="utf-8")
            self.assertGreater(original["bytes"] + payload_size, payload_size)
        arguments = SimpleNamespace(
            benchmark_report=self.baseline,
            trace_directory=trace_directory,
            loader=self.loader,
            direct_mapping=False,
        )
        return m2.build_report(arguments)

    def test_repository_schema_is_valid_json(self):
        schema = self.workspace / "benchmarks/schema/m2-performance-report-v2.schema.json"
        value = json.loads(schema.read_text(encoding="utf-8"))
        self.assertEqual(value["$schema"], "https://json-schema.org/draft/2020-12/schema")
        self.assertEqual(value["properties"]["schema_version"]["const"], 2)

    def test_builds_v1_baseline_with_explicit_failed_m2_gates(self):
        with tempfile.TemporaryDirectory() as directory:
            report = self.build_baseline(Path(directory))
        self.assertEqual(report["schema_version"], 2)
        self.assertEqual(report["loader"]["bytes"], 10_112)
        self.assertEqual(report["loader"]["runtime_codec"], "lz4")
        self.assertTrue(all(fixture["codec"] == "lz4" for fixture in report["fixtures"]))
        self.assertFalse(report["gates"]["size_win_pass"])
        self.assertFalse(report["gates"]["cold_win_pass"])
        self.assertFalse(report["gates"]["direct_mapping_pass"])
        self.assertFalse(report["gates"]["no_memfd_pass"])
        self.assertFalse(report["gates"]["no_secondary_exec_pass"])
        self.assertFalse(report["gates"]["release_pass"])
        self.assertTrue(all(fixture["phase_timings_ns"] is None for fixture in report["fixtures"]))

    def test_release_requires_same_report_to_pass_every_gate(self):
        with tempfile.TemporaryDirectory() as directory:
            report = self.build_baseline(Path(directory))
        passing = copy.deepcopy(report)
        passing["loader"]["direct_mapping"] = True
        for fixture in passing["fixtures"]:
            fixture["size_packforge_over_upx_basis_points"] = 9_500
            fixture["cold_packforge_over_upx_basis_points"] = 9_000
            fixture["rss_packforge_over_upx_basis_points"] = 10_000
            fixture["syscalls"]["memfd_create"] = 0
            fixture["syscalls"]["execve"] = 1
            fixture["syscalls"]["execveat"] = 0
        passing["gates"] = m2.calculate_gates(passing["fixtures"], passing["loader"])
        self.assertTrue(passing["gates"]["release_pass"])
        m2.validate_report(passing)

        passing["gates"]["release_pass"] = False
        with self.assertRaises(m2.ContractError):
            m2.validate_report(passing)

    def test_trace_parser_rejects_missing_initial_exec(self):
        with tempfile.TemporaryDirectory() as directory:
            trace = Path(directory) / "bad.strace"
            trace.write_text("memfd_create(\"packforge\", 3) = 4\n", encoding="utf-8")
            with self.assertRaises(m2.ContractError):
                m2.parse_trace(trace)

    def test_reads_direct_load_v2_payload_metadata(self):
        codec, payload_size, decoder_memory = m2.inspect_payload(
            {
                "artifact_kind": "executable",
                "executable_version": 2,
                "codec": 5,
                "payload_size": 123_456,
            },
            "fixture",
        )
        self.assertEqual(codec, "apultra-bcj2")
        self.assertEqual(payload_size, 123_456)
        self.assertIsNone(decoder_memory)


if __name__ == "__main__":
    unittest.main()
