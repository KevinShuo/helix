"""Regression checks for benchmark failure cleanup and evidence classification."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest import mock

SPEC = importlib.util.spec_from_file_location("local_performance", Path(__file__).with_name("ai-local-performance.py"))
assert SPEC and SPEC.loader
BENCH = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(BENCH)


class PerformanceHarnessTests(unittest.TestCase):
    def test_metadata_write_failure_reaps_real_started_child(self):
        child = subprocess.Popen(
            [sys.executable, "-c", "import time; time.sleep(60)"],
            stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            creationflags=BENCH.NO_WINDOW,
        )
        original_write = BENCH.write_json

        def fail_metadata(path, value):
            if path.name == "process.json":
                raise OSError("synthetic metadata write failure")
            original_write(path, value)

        try:
            with tempfile.TemporaryDirectory() as directory, \
                 mock.patch.object(BENCH, "ensure_idle"), \
                 mock.patch.object(BENCH.subprocess, "Popen", return_value=child), \
                 mock.patch.object(BENCH, "write_json", side_effect=fail_metadata):
                with self.assertRaisesRegex(OSError, "metadata write failure"):
                    with BENCH.server(Path("unused-server"), "27b", [], Path(directory), None):
                        self.fail("server must not become ready")
            self.assertIsNotNone(child.poll(), "failed metadata write must reap the child")
        finally:
            if child.poll() is None:
                child.terminate()
                child.wait(timeout=10)

    def test_schema_valid_wrong_production_selection_is_rejected(self):
        def fake_evaluator(command, **kwargs):
            target = Path(kwargs["env"]["HELIX_AI_EVAL_OUTPUT"])
            rows = [{"id": f"case-{i}", "error": None, "elapsed_ms": 100,
                     "input": {"expected_selected": [1]}, "result": {"selected": [2]}}
                    for i in range(6)]
            target.write_text("\n".join(json.dumps(row) for row in rows), encoding="utf-8")
            return subprocess.CompletedProcess(command, 0)

        with tempfile.TemporaryDirectory() as directory, \
             mock.patch.object(BENCH.subprocess, "run", side_effect=fake_evaluator):
            with self.assertRaisesRegex(ValueError, "selection"):
                BENCH.production_evaluate(Path("unused-test-binary"), "http://127.0.0.1:1", "27b", Path(directory))


if __name__ == "__main__":
    unittest.main()
