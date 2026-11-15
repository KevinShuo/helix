"""Real framework regression tests for the bundled Helix pytest adapter."""

import json
import subprocess
import sys
from pathlib import Path

BRIDGE = (
    Path(__file__).resolve().parents[1]
    / "helix-term/src/handlers/test/pytest_bridge.py"
)


def execute(tmp_path, scope):
    request = tmp_path / "request.json"
    report = tmp_path / "report.jsonl"
    request.write_text(json.dumps(scope), encoding="utf-8")
    result = subprocess.run(
        [sys.executable, str(BRIDGE), str(request), str(report)],
        cwd=tmp_path,
        capture_output=True,
        text=True,
        timeout=30,
    )
    assert report.is_file(), result.stderr
    events = [
        json.loads(line) for line in report.read_text(encoding="utf-8").splitlines()
    ]
    return result, events


def test_structured_outcomes_and_failed_only_rerun(tmp_path):
    (tmp_path / "test_cases.py").write_text(
        """import pytest
import unittest
@pytest.fixture
def teardown_error():
    yield
    raise RuntimeError("broken cleanup")
def test_pass(): assert True
def test_bad(): assert False, "bad value"
def test_error(teardown_error): pass
@pytest.mark.skip(reason="later")
def test_skip(): pass
@pytest.mark.parametrize("value", [1, 2], ids=["a::b", "a[b]"])
def test_param(value): assert value > 0
class MyCase(unittest.TestCase):
    def test_works(self): self.assertEqual(1, 1)
""",
        encoding="utf-8",
    )
    result, events = execute(tmp_path, {"args": ["test_cases.py"]})
    outcomes = [e["case"] for e in events if e["event"] == "result"]
    assert result.returncode == 1
    assert len(outcomes) == 7
    states = {c["name"]: c["outcome"] for c in outcomes}
    assert states["test_pass"] == "passed"
    assert states["test_skip"] == "skipped"
    assert states["test_error"] == "error"
    failed = [c["id"] for c in outcomes if c["outcome"] in ("failed", "error")]
    _, rerun = execute(tmp_path, {"args": failed})
    assert {e["case"]["id"] for e in rerun if e["event"] == "result"} == set(failed)


def test_declaration_selection_parameter_ids_and_empty_collection(tmp_path):
    (tmp_path / "test_cases.py").write_text(
        """import pytest
@pytest.mark.parametrize("x", [1, 2], ids=["a::b", "a[b]"])
def test_one(x): assert x > 0
def test_other(): assert False
""",
        encoding="utf-8",
    )
    result, events = execute(tmp_path, {"args": ["test_cases.py::test_one"]})
    ids = [e["case"]["id"] for e in events if e["event"] == "result"]
    assert result.returncode == 0 and len(ids) == 2
    result, events = execute(tmp_path, {"args": [ids[0]]})
    assert result.returncode == 0
    assert [e["case"]["id"] for e in events if e["event"] == "result"] == [ids[0]]
    result, _ = execute(tmp_path, {"args": ["test_cases.py::missing"]})
    assert result.returncode != 0


def test_pytest_root_above_run_directory_has_absolute_paths_and_rerun_selector(
    tmp_path,
):
    (tmp_path / "pytest.ini").write_text("[pytest]\n", encoding="utf-8")
    child = tmp_path / "pkg"
    child.mkdir()
    (child / "pyproject.toml").write_text(
        "[project]\nname='fixture'\nversion='1'\n", encoding="utf-8"
    )
    source = child / "test_nested.py"
    source.write_text("def test_bad(): assert False\n", encoding="utf-8")
    _, events = execute(child, {"args": [str(source)]})
    case = next(e["case"] for e in events if e["event"] == "result")
    assert Path(case["path"]) == source
    assert Path(case["failure_path"]) == source
    _, rerun = execute(child, {"args": [case["selector"]]})
    assert [e["case"]["id"] for e in rerun if e["event"] == "result"] == [case["id"]]


def test_deselected_tests_are_not_queued_or_failed(tmp_path):
    (tmp_path / "pytest.ini").write_text(
        "[pytest]\naddopts = -k test_one\n", encoding="utf-8"
    )
    (tmp_path / "test_cases.py").write_text(
        "def test_one(): pass\ndef test_two(): pass\n", encoding="utf-8"
    )
    result, events = execute(tmp_path, {"args": ["test_cases.py"]})
    assert result.returncode == 0
    collected = [e["case"]["id"] for e in events if e["event"] == "collected"]
    assert collected == ["test_cases.py::test_one"]
