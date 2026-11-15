#!/usr/bin/env python3
"""Evaluate real configured local 27B thinking and DeepSeek on fixed test tasks.

Only synthetic fixtures are transmitted. Run controls before model requests;
retain first-shot failures without retries, coercion or code edits.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import statistics
import subprocess
import time
import tomllib
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
TEST = "handlers::ai_test::client::evaluation::ai_test_quality_evaluation"
NO_WINDOW = 0x08000000 if os.name == "nt" else 0


def module(name: str, path: Path) -> Any:
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec and spec.loader
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


def write_json(path: Path, value: Any) -> None:
    path.write_text(
        json.dumps(value, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )


def rows(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    return [
        json.loads(line)
        for line in path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    ]


async def evaluate(
    binary: Path, arm: str, output: Path, work: Path, python: Path
) -> None:
    target = output / f"{arm}.jsonl"
    if target.exists():
        raise FileExistsError(f"Refusing to mix observations: {target}")
    environment = os.environ.copy()
    environment.update(
        {
            "CARGO_MANIFEST_DIR": str(ROOT / "helix-term"),
            "HELIX_RUNTIME": str(ROOT / "runtime"),
            "HELIX_AI_TEST_EVAL_ARM": arm,
            "HELIX_AI_TEST_EVAL_OUTPUT": str(output),
            "HELIX_AI_TEST_EVAL_WORK": str(work),
            "HELIX_AI_TEST_PYTHON": str(python),
            "PYTHONDONTWRITEBYTECODE": "1",
        }
    )
    with (output / f"{arm}.test.log").open("w", encoding="utf-8") as log:
        process = await asyncio.create_subprocess_exec(
            str(binary),
            "--ignored",
            "--exact",
            TEST,
            "--nocapture",
            "--test-threads=1",
            env=environment,
            cwd=ROOT,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
            creationflags=NO_WINDOW,
        )
        reported = 0
        started = time.monotonic()
        try:
            while process.returncode is None:
                observations = rows(target)
                for row in observations[reported:]:
                    killed = sum(m["killed"] for m in row["mutants"])
                    baseline = row["baseline"] or {}
                    print(
                        f"{arm}/{row['case']}: accepted={row['accepted']} pass={baseline.get('success')} generation={row['generation_ms']}ms mutants={killed}/3 error={row['generation_error']}",
                        flush=True,
                    )
                reported = len(observations)
                if time.monotonic() - started > 2400:
                    raise TimeoutError(f"Evaluation timed out: {arm}")
                await asyncio.sleep(0.5)
            if process.returncode:
                raise RuntimeError(
                    f"Evaluation failed: {arm}; inspect {output / f'{arm}.test.log'}"
                )
            observations = rows(target)
            if len(observations) != 6:
                raise RuntimeError(
                    f"Expected 6 observations: {arm}, got {len(observations)}"
                )
            for row in observations[reported:]:
                print(
                    f"{arm}/{row['case']}: pass={(row['baseline'] or {}).get('success')} generation={row['generation_ms']}ms",
                    flush=True,
                )
        finally:
            if process.returncode is None:
                process.terminate()
                await process.wait()


def summarize(output: Path) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for arm in ["27b-thinking", "deepseek"]:
        observations = rows(output / f"{arm}.jsonl")
        result[arm] = {
            "observations": len(observations),
            "accepted": sum(r["accepted"] for r in observations),
            "baseline_pass": sum(
                bool((r["baseline"] or {}).get("success")) for r in observations
            ),
            "median_generation_ms": statistics.median(
                r["generation_ms"] for r in observations
            ),
            "total_generation_ms": sum(r["generation_ms"] for r in observations),
            "mutants_killed": sum(
                m["killed"] for r in observations for m in r["mutants"]
            ),
            "mutants_exercised": sum(len(r["mutants"]) for r in observations),
            "mutants_available": 18,
            "cases": [
                {
                    "case": r["case"],
                    "language": r["language"],
                    "generation_ms": r["generation_ms"],
                    "accepted": r["accepted"],
                    "baseline_pass": (r["baseline"] or {}).get("success", False),
                    "mutants_killed": sum(m["killed"] for m in r["mutants"]),
                    "baseline_ms": (r["baseline"] or {}).get("elapsed_ms"),
                    "generation_error": r["generation_error"],
                }
                for r in observations
            ],
        }
    identical = {}
    for case in json.loads((output / "cases.json").read_text(encoding="utf-8")):
        messages = []
        for arm in ["27b-thinking", "deepseek"]:
            request = json.loads(
                (output / arm / case["id"] / "request.json").read_text(encoding="utf-8")
            )
            messages.append(
                hashlib.sha256(
                    json.dumps(
                        request["messages"], ensure_ascii=False, sort_keys=True
                    ).encode()
                ).hexdigest()
            )
        identical[case["id"]] = {
            "identical": messages[0] == messages[1],
            "hashes": messages,
        }
    result["prompt_equivalence"] = identical
    write_json(output / "summary.json", result)
    return result


async def run(args: argparse.Namespace) -> None:
    output, work = args.output.resolve(), args.work.resolve()
    output.mkdir(parents=True, exist_ok=True)
    work.mkdir(parents=True, exist_ok=True)
    fixture_module = module(
        "ai_test_eval_fixtures", ROOT / "contrib/ai_test_eval_fixtures.py"
    )
    cases = fixture_module.materialize()
    fixture_path = output / "cases.json"
    if fixture_path.exists():
        assert json.loads(fixture_path.read_text(encoding="utf-8")) == cases, (
            "Fixtures changed"
        )
    else:
        write_json(fixture_path, cases)
    config = tomllib.loads(
        (Path(os.environ["APPDATA"]) / "helix/config.toml").read_text(
            encoding="utf-8-sig"
        )
    )["editor"]
    local = dict(config["ai"]["providers"]["local-27b"])
    cloud = dict(config["ai"]["providers"]["deepseek"])
    if not os.environ.get(cloud["api-key-env"]):
        raise RuntimeError("DeepSeek API key environment variable is missing")
    server = Path(os.path.expandvars(local["server-path"]))
    weights = Path(os.path.expandvars(local["model-path"]))
    for path in [args.test_binary, args.python, server, weights]:
        if not path.is_file():
            raise FileNotFoundError(path)
    support = module("generation_eval_support", ROOT / "contrib/ai-generation-eval.py")
    support.ensure_idle()
    manifest: dict[str, Any] = {
        "cases": 6,
        "mutants_per_case": 3,
        "repetitions": 1,
        "first_shot": True,
        "automatic_repairs": 0,
        "context_size": 16384,
        "final_tokens": 4096,
        "local_thinking_budget": local.get("thinking-budget"),
        "local_thinking_effort": local.get("reasoning-effort"),
        "deepseek_thinking": cloud.get("thinking", False),
        "request_timeout_ms": 120000,
        "local_model": local["model"],
        "deepseek_model": cloud["model"],
        "weights_size_bytes": weights.stat().st_size,
        "sources": {},
        "load_ms": None,
    }
    for path in [
        ROOT / "contrib/ai-test-generation-eval.py",
        ROOT / "contrib/ai_test_eval_fixtures.py",
        ROOT / "helix-term/src/handlers/ai_test/evaluation.rs",
        ROOT / "helix-term/src/handlers/ai_test/client.rs",
        ROOT / "helix-term/src/handlers/ai_test/project.rs",
        ROOT / "helix-term/src/handlers/ai_test/runner.rs",
        fixture_path,
        args.test_binary,
    ]:
        manifest["sources"][str(path.resolve().relative_to(ROOT))] = hashlib.sha256(
            path.read_bytes()
        ).hexdigest()
    write_json(output / "manifest.json", manifest)
    print("Validating independent gold tests and all 18 mutations first...", flush=True)
    await evaluate(
        args.test_binary.resolve(), "gold", output, work, args.python.resolve()
    )
    print("Gold controls passed. Loading local 27B thinking...", flush=True)
    async with support.model_server(
        server, weights, local["model"], output / "27b.server.log"
    ) as (address, load_ms):
        manifest["load_ms"] = load_ms
        local.pop("model-path", None)
        local["endpoint"] = address + "/v1/chat/completions"
        local["context-size"] = 16384
        write_json(output / "27b-thinking.provider.json", local)
        write_json(output / "manifest.json", manifest)
        print(f"Local model ready, load={load_ms:.0f}ms", flush=True)
        await evaluate(
            args.test_binary.resolve(),
            "27b-thinking",
            output,
            work,
            args.python.resolve(),
        )
    cloud["context-size"] = 16384
    write_json(output / "deepseek.provider.json", cloud)
    print("Local model unloaded. Running configured DeepSeek...", flush=True)
    await evaluate(
        args.test_binary.resolve(), "deepseek", output, work, args.python.resolve()
    )
    result = summarize(output)
    assert all(v["identical"] for v in result["prompt_equivalence"].values()), (
        "Prompts differed; do not treat arms as matched"
    )
    print(
        json.dumps(
            {
                arm: {k: v for k, v in data.items() if k != "cases"}
                for arm, data in result.items()
                if arm != "prompt_equivalence"
            },
            ensure_ascii=False,
            indent=2,
        ),
        flush=True,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--work", type=Path, default=ROOT / ".superpowers/ai-test-eval/projects"
    )
    parser.add_argument(
        "--python",
        type=Path,
        default=ROOT / ".superpowers/ai-test/venv/Scripts/python.exe",
    )
    asyncio.run(run(parser.parse_args()))


if __name__ == "__main__":
    main()
