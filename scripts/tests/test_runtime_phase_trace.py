import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).resolve().parents[1] / "runtime_phase_trace.py"
SPEC = importlib.util.spec_from_file_location("runtime_phase_trace", SCRIPT)
runtime_phase_trace = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(runtime_phase_trace)


TRACE = """\
42 100.000000 execve("/tmp/packed", ["/tmp/packed"], 0x0) = 0 <0.000010>
42 100.000020 openat(AT_FDCWD, "/proc/self/exe", O_RDONLY) = 3 <0.000002>
42 100.000030 pread64(3, "trailer", 128, 1000) = 128 <0.000003>
42 100.000040 mmap(NULL, 20000, PROT_READ|PROT_WRITE, MAP_ANONYMOUS, -1, 0) = 0x900000 <0.000004>
42 100.000050 pread64(3, "loader", 20000, 0) = 20000 <0.000005>
42 100.000060 pread64(3, "header", 192, 20000) = 192 <0.000003>
42 100.000070 pread64(3, "manifest", 232, 20192) = 232 <0.000003>
42 100.000080 mmap(NULL, 200000, PROT_READ|PROT_WRITE, MAP_ANONYMOUS, -1, 0) = 0xa00000 <0.000004>
42 100.000090 pread64(3, "payload", 200000, 20424) = 200000 <0.000006>
42 100.000110 mmap(NULL, 500000, PROT_READ|PROT_WRITE, MAP_ANONYMOUS, -1, 0) = 0xb00000 <0.000005>
42 100.000150 mmap(0x400000, 4096, PROT_NONE, MAP_ANONYMOUS|MAP_FIXED_NOREPLACE, -1, 0) = 0x400000 <0.000004>
42 100.000160 mmap(0x401000, 8192, PROT_NONE, MAP_ANONYMOUS|MAP_FIXED_NOREPLACE, -1, 0) = 0x401000 <0.000004>
42 100.000170 mprotect(0x400000, 4096, PROT_READ|PROT_WRITE) = 0 <0.000003>
42 100.000180 mprotect(0x400000, 4096, PROT_READ) = 0 <0.000003>
42 100.000190 mprotect(0x401000, 8192, PROT_READ|PROT_WRITE) = 0 <0.000003>
42 100.000200 mprotect(0x401000, 8192, PROT_READ|PROT_EXEC) = 0 <0.000003>
42 100.000220 mmap(NULL, 65536, PROT_READ|PROT_WRITE, MAP_ANONYMOUS, -1, 0) = 0xc00000 <0.000004>
42 100.000230 pread64(3, "target-runtime", 64, 0) = 12 <0.000003>
43 100.000240 write(1, "ok", 2) = 2 <0.000002>
"""


class RuntimePhaseTraceTests(unittest.TestCase):
    def test_extracts_loader_boundaries_and_ignores_target_mmap(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trace"
            path.write_text(TRACE, encoding="utf-8")
            self.assertEqual(
                runtime_phase_trace.parse_trace(path),
                {
                    "payload_read": 6_000,
                    "payload_hash": 14_000,
                    "decompress": 35_000,
                    "map_segments": 53_000,
                    "transfer": 37_000,
                },
            )

    def test_rejects_trace_without_timestamps(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "trace"
            path.write_text('42 execve("/tmp/packed", [], 0x0) = 0\n', encoding="utf-8")
            with self.assertRaises(runtime_phase_trace.TraceError):
                runtime_phase_trace.parse_trace(path)


if __name__ == "__main__":
    unittest.main()
