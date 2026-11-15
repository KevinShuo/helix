#!/usr/bin/env python3
"""Replay unchanged accepted model outputs with a common UTF-8 MSVC toolchain.

Original observations remain immutable. No model request or generated-test edit.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
NO_WINDOW = 0x08000000 if os.name == "nt" else 0


def execute(argv: list[str], cwd: Path, stage: str) -> dict[str, Any]:
    environment = {**os.environ, "PYTHONDONTWRITEBYTECODE": "1"}
    started = time.monotonic()
    process = subprocess.Popen(
        argv,
        cwd=cwd,
        env=environment,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        creationflags=NO_WINDOW,
    )
    timed_out = False
    try:
        output, _ = process.communicate(timeout=180)
    except subprocess.TimeoutExpired:
        timed_out = True
        subprocess.run(
            ["taskkill.exe", "/PID", str(process.pid), "/T", "/F"],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            creationflags=NO_WINDOW,
            check=False,
        )
        output, _ = process.communicate()
    return {
        "argv": argv,
        "stage": stage,
        "exit_code": process.returncode,
        "success": process.returncode == 0 and not timed_out,
        "timeout": timed_out,
        "elapsed_ms": round((time.monotonic() - started) * 1000),
        "log": output.decode("utf-8", errors="replace")[-65536:],
    }


def suite_run(
    root: Path,
    build: Path,
    target: str | None,
    suite: str,
    test: Path,
    python: Path,
    configure: bool,
) -> dict[str, Any]:
    commands = []
    if target:
        if configure:
            commands.append(
                (
                    [
                        "cmake",
                        "-S",
                        str(root),
                        "-B",
                        str(build),
                        "-DBUILD_TESTING=ON",
                        "-DCMAKE_CXX_FLAGS=/utf-8",
                        f"-DCMAKE_TOOLCHAIN_FILE={Path(os.environ['VCPKG_ROOT']) / 'scripts/buildsystems/vcpkg.cmake'}",
                    ],
                    "Configure",
                )
            )
        commands.extend(
            [
                (
                    [
                        "cmake",
                        "--build",
                        str(build),
                        "--config",
                        "Debug",
                        "--target",
                        target,
                    ],
                    "Build",
                ),
                (
                    [
                        "ctest",
                        "--test-dir",
                        str(build),
                        "-C",
                        "Debug",
                        "--output-on-failure",
                        "--no-tests=error",
                        "-R",
                        suite,
                    ],
                    "Test",
                ),
            ]
        )
    else:
        commands.append(
            (
                [str(python), "-m", "pytest", "-q", "-rA", "--color=no", str(test)],
                "Test",
            )
        )
    steps = []
    for argv, stage in commands:
        step = execute(argv, root, stage)
        steps.append(step)
        if not step["success"]:
            break
    return {
        "success": steps[-1]["success"],
        "stage": steps[-1]["stage"],
        "steps": steps,
        "elapsed_ms": sum(s["elapsed_ms"] for s in steps),
    }


def replay(
    output: Path, work: Path, python: Path, include_rejected: bool = False
) -> dict[str, Any]:
    cases = json.loads((output / "cases.json").read_text(encoding="utf-8"))
    results = {}
    for arm in ["27b-thinking", "deepseek"]:
        observations = []
        for case in cases:
            original_root = (
                ROOT / ".superpowers/ai-test-eval/projects" / arm / case["id"]
            )
            primary = json.loads(
                (output / arm / case["id"] / "result.json").read_text(encoding="utf-8")
            )
            row: dict[str, Any] = {
                "arm": arm,
                "case": case["id"],
                "language": case["language"],
                "primary_generation_ms": primary["generation_ms"],
                "baseline": None,
                "mutants": [],
                "unchanged_test_sha256": None,
                "generation_error": primary["generation_error"],
                "primary_accepted": primary["accepted"],
                "validation_bypassed": False,
            }
            raw_code = None
            response_file = output / arm / case["id"] / "response.json"
            if (
                not primary["accepted"]
                and include_rejected
                and case["language"] == "python"
                and response_file.exists()
            ):
                response = json.loads(response_file.read_text(encoding="utf-8"))
                raw_code = json.loads(response["choices"][0]["message"]["content"])[
                    "code"
                ].encode("utf-8")
                row["validation_bypassed"] = True
            if not primary["accepted"] and raw_code is None:
                observations.append(row)
                continue
            root = work / arm / case["id"]
            root.mkdir(parents=True, exist_ok=True)
            for name, source in case["files"].items():
                (root / name).parent.mkdir(parents=True, exist_ok=True)
                (root / name).write_text(source, encoding="utf-8")
            request = json.loads(
                (output / arm / case["id"] / "request.json").read_text(encoding="utf-8")
            )
            metadata = json.loads(
                request["messages"][1]["content"]
                .splitlines()[0]
                .removeprefix("Target: ")
            )
            target = None
            for file in [
                original_root / "CMakeLists.txt",
                *original_root.glob("tests/**/CMakeLists.txt"),
            ]:
                if not file.exists():
                    continue
                text = file.read_text(encoding="utf-8").replace(
                    str(original_root).replace("\\", "/"), str(root).replace("\\", "/")
                )
                path = root / file.relative_to(original_root)
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(text, encoding="utf-8")
                match = re.search(r"add_executable\((\S+)", text)
                if match:
                    target = match.group(1)
            test = root / metadata["test_file"]
            test.parent.mkdir(parents=True, exist_ok=True)
            original_test = (
                raw_code
                if raw_code is not None
                else (original_root / metadata["test_file"]).read_bytes()
            )
            test.write_bytes(original_test)
            row["unchanged_test_sha256"] = hashlib.sha256(original_test).hexdigest()
            assert (
                hashlib.sha256(test.read_bytes()).hexdigest()
                == row["unchanged_test_sha256"]
            )
            build = root / ".utf8-build"
            row["baseline"] = suite_run(
                root, build, target, metadata["test_suite"], test, python, True
            )
            if row["baseline"]["success"]:
                for mutant in case["mutants"]:
                    source = case["files"][case["source_file"]]
                    (root / case["source_file"]).write_text(
                        source.replace(mutant["old"], mutant["new"], 1),
                        encoding="utf-8",
                    )
                    run = suite_run(
                        root, build, target, metadata["test_suite"], test, python, False
                    )
                    killed = (
                        not run["success"]
                        and run["stage"] == "Test"
                        and not any(s["timeout"] for s in run["steps"])
                    )
                    row["mutants"].append(
                        {
                            "id": mutant["id"],
                            "meaning": mutant["meaning"],
                            "killed": killed,
                            "run": run,
                        }
                    )
                (root / case["source_file"]).write_text(
                    case["files"][case["source_file"]], encoding="utf-8"
                )
                row["restored"] = suite_run(
                    root, build, target, metadata["test_suite"], test, python, False
                )
                assert row["restored"]["success"], (
                    arm,
                    case["id"],
                    "failed restoration",
                )
            dest = output / "utf8-replay" / arm / case["id"]
            dest.mkdir(parents=True, exist_ok=True)
            (dest / "result.json").write_text(
                json.dumps(row, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
            )
            observations.append(row)
            print(
                f"UTF8 {arm}/{case['id']}: pass={row['baseline']['success']} mutants={sum(m['killed'] for m in row['mutants'])}/3",
                flush=True,
            )
        results[arm] = observations
    (output / "utf8-replay-summary.json").write_text(
        json.dumps(results, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    return results


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--work", type=Path, default=ROOT / ".superpowers/ai-test-eval/utf8-replay"
    )
    parser.add_argument(
        "--python",
        type=Path,
        default=ROOT / ".superpowers/ai-test/venv/Scripts/python.exe",
    )
    parser.add_argument(
        "--include-rejected",
        action="store_true",
        help="Separately compile raw Python code rejected by response-envelope validation",
    )
    args = parser.parse_args()
    replay(
        args.output.resolve(),
        args.work.resolve(),
        args.python.resolve(),
        args.include_rejected,
    )


if __name__ == "__main__":
    main()
