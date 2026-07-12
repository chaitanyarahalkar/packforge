from __future__ import annotations

import struct
import subprocess
import tempfile
import unittest
from pathlib import Path


WORKSPACE = Path(__file__).resolve().parents[2]
CHECKER = WORKSPACE / "scripts" / "check-runtime-v2-elf.py"
LOADER = WORKSPACE / "runtime" / "artifacts" / "linux-x86_64" / "loader-v2"


class RuntimeV2ElfTests(unittest.TestCase):
    def run_checker(self, data: bytes) -> subprocess.CompletedProcess[str]:
        with tempfile.NamedTemporaryFile() as artifact:
            artifact.write(data)
            artifact.flush()
            return subprocess.run(
                ["python3", str(CHECKER), artifact.name],
                text=True,
                capture_output=True,
                check=False,
            )

    def test_accepts_checked_in_loader(self) -> None:
        result = self.run_checker(LOADER.read_bytes())
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_rejects_executable_type_and_writable_executable_load(self) -> None:
        executable = bytearray(LOADER.read_bytes())
        struct.pack_into("<H", executable, 16, 2)
        self.assertNotEqual(self.run_checker(executable).returncode, 0)

        writable_executable = bytearray(LOADER.read_bytes())
        phoff = struct.unpack_from("<Q", writable_executable, 32)[0]
        phentsize, phnum = struct.unpack_from("<HH", writable_executable, 54)
        for index in range(phnum):
            offset = phoff + index * phentsize
            if struct.unpack_from("<I", writable_executable, offset)[0] == 1:
                struct.pack_into("<I", writable_executable, offset + 4, 7)
                break
        self.assertNotEqual(self.run_checker(writable_executable).returncode, 0)


if __name__ == "__main__":
    unittest.main()
