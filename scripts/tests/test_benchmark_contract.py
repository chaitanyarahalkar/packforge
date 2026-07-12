import importlib.util
import json
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace


SCRIPT = Path(__file__).parents[1] / "benchmark_contract.py"
SPEC = importlib.util.spec_from_file_location("benchmark_contract", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
benchmark_contract = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(benchmark_contract)


class BenchmarkContractTests(unittest.TestCase):
    def test_integer_median_matches_rust_contract(self):
        self.assertEqual(benchmark_contract.integer_median([]), 0)
        self.assertEqual(benchmark_contract.integer_median([9]), 9)
        self.assertEqual(benchmark_contract.integer_median([9, 3, 6]), 6)
        self.assertEqual(benchmark_contract.integer_median([2, 1]), 1)

    def test_calculates_passing_and_failing_gates(self):
        def fixture(identifier, packforge_size, upx_size, packforge_cold, upx_cold):
            def artifact(kind, size, cold):
                return {
                    "kind": kind,
                    "bytes": size,
                    "behavior_matches_original": True,
                    "reversible": kind != "upx",
                    "deterministic": True,
                    "cold_time_ns": {"median": cold, "values": [cold]},
                }

            return {
                "id": identifier,
                "artifacts": [
                    artifact("original", 1_000, 10),
                    artifact("packforge", packforge_size, packforge_cold),
                    artifact("upx", upx_size, upx_cold),
                ],
            }

        passing = benchmark_contract.calculate_gates([fixture("a", 100, 100, 90, 100)])
        self.assertTrue(passing["release_pass"])
        failing = benchmark_contract.calculate_gates([fixture("a", 160, 100, 120, 100)])
        self.assertFalse(failing["size_pass"])
        self.assertFalse(failing["cold_pass"])
        self.assertFalse(failing["release_pass"])

    def test_rejects_changed_corpus_source(self):
        with tempfile.TemporaryDirectory() as directory:
            workspace = Path(directory)
            source = workspace / "fixture.c"
            source.write_text("int main(void) { return 0; }\n", encoding="utf-8")
            corpus = {
                "schema_version": 1,
                "license": "MIT",
                "fixtures": [
                    {
                        "id": "fixture",
                        "language": "c",
                        "source": "fixture.c",
                        "source_sha256": "0" * 64,
                        "target": "test",
                        "compiler": "cc",
                        "arguments": [],
                    }
                ],
            }
            corpus_path = workspace / "corpus.json"
            corpus_path.write_text(json.dumps(corpus), encoding="utf-8")
            with self.assertRaises(benchmark_contract.ContractError):
                benchmark_contract.validate_corpus(workspace, corpus_path)

    def test_repository_schema_is_valid_json(self):
        schema = Path(__file__).parents[2] / "benchmarks/schema/benchmark-report-v1.schema.json"
        with schema.open(encoding="utf-8") as stream:
            value = json.load(stream)
        self.assertEqual(value["$schema"], "https://json-schema.org/draft/2020-12/schema")

    def test_checked_in_native_report_revalidates(self):
        report_path = Path(__file__).parents[2] / (
            "benchmarks/results/m0-linux-x86_64-2026-07-11/report.json"
        )
        with report_path.open(encoding="utf-8") as stream:
            report = json.load(stream)
        benchmark_contract.validate_report(report)
        report["gates"]["release_pass"] = True
        with self.assertRaises(benchmark_contract.ContractError):
            benchmark_contract.validate_report(report)

    def test_checked_in_samples_reaggregate_deterministically(self):
        workspace = Path(__file__).parents[2]
        result = workspace / "benchmarks/results/m0-linux-x86_64-2026-07-11"
        arguments = SimpleNamespace(
            workspace=workspace,
            corpus=workspace / "benchmarks/corpus-v1.json",
            summary=result / "summary.tsv",
            raw=result / "raw-samples.tsv",
            metadata=result / "metadata.json",
            packforge_profile="fast",
            cold_cache_reset="linux_drop_caches_3",
        )
        first = benchmark_contract.build_report(arguments)
        second = benchmark_contract.build_report(arguments)
        self.assertEqual(first, second)
        self.assertEqual(
            json.dumps(first, indent=2) + "\n",
            (result / "report.json").read_text(encoding="utf-8"),
        )


if __name__ == "__main__":
    unittest.main()
