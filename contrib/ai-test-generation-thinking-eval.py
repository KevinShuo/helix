#!/usr/bin/env python3
"""Full six-case DeepSeek thinking comparison with a larger total output budget.

Only the ignored evaluation harness overrides max_tokens. Daily configuration,
production output limits, prompts and generated tests remain unchanged.
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
import tomllib
from typing import Any

ROOT = Path(__file__).resolve().parents[1]
ARM = "deepseek-thinking-16k"


def load(path: Path) -> Any:
    spec = importlib.util.spec_from_file_location(path.stem, path)
    assert spec and spec.loader
    value = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(value)
    return value


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


async def run(args: argparse.Namespace) -> None:
    output = args.output.resolve()
    reference = args.reference.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if (output / f"{ARM}.jsonl").exists():
        raise FileExistsError("Refusing to overwrite previous observations")
    config_path = Path(os.environ["APPDATA"]) / "helix/config.toml"
    config_before = sha(config_path)
    provider = dict(
        tomllib.loads(config_path.read_text(encoding="utf-8-sig"))["editor"]["ai"][
            "providers"
        ]["deepseek"]
    )
    if not os.environ.get(provider["api-key-env"]):
        raise RuntimeError("DeepSeek API key environment variable is missing")
    provider["thinking"] = True
    provider["context-size"] = 32768
    common = load(ROOT / "contrib/ai-test-generation-eval.py")
    fixture_module = load(ROOT / "contrib/ai_test_eval_fixtures.py")
    cases = fixture_module.materialize()
    assert cases == json.loads((reference / "cases.json").read_text(encoding="utf-8"))
    common.write_json(output / "cases.json", cases)
    common.write_json(output / f"{ARM}.provider.json", provider)
    manifest: dict[str, Any] = {
        "arm": ARM,
        "model": provider["model"],
        "thinking": True,
        "reasoning_effort": "API default high; not overridden",
        "total_output_budget": 16384,
        "context_size": 32768,
        "request_timeout_ms": 120000,
        "repetitions": 1,
        "automatic_repairs": 0,
        "cases": 6,
        "mutants": 18,
        "daily_config_modified": False,
        "evaluation_only_override": True,
        "production_feature_budget": 4096,
        "production_feature_limit": 8192,
        "reference": str(reference.relative_to(ROOT)),
        "case_sha256": sha(output / "cases.json"),
        "official_parameter_docs": [
            "https://api-docs.deepseek.com/api/create-chat-completion/",
            "https://api-docs.deepseek.com/guides/thinking_mode/",
        ],
        "sources": {},
    }
    for name in [
        "contrib/ai-test-generation-thinking-eval.py",
        "contrib/ai-test-generation-eval.py",
        "contrib/ai_test_eval_fixtures.py",
        "helix-term/src/handlers/ai_test/evaluation.rs",
        "helix-term/src/handlers/ai_test/client.rs",
        "helix-term/src/handlers/ai_test/project.rs",
        "helix-term/src/handlers/ai_test/runner.rs",
        "helix-term/src/handlers/ai.rs",
    ]:
        source = ROOT / name
        manifest["sources"][name] = sha(source)
        snapshot = output / "source-snapshot" / name
        snapshot.parent.mkdir(parents=True, exist_ok=True)
        snapshot.write_bytes(source.read_bytes())
    manifest["test_binary_sha256"] = sha(args.test_binary)
    common.write_json(output / "manifest.json", manifest)
    os.environ["HELIX_AI_TEST_EVAL_OUTPUT_TOKENS"] = "16384"
    os.environ["HELIX_AI_TEST_EVAL_REFERENCE"] = str(reference / "deepseek")
    os.environ.pop("HELIX_AI_TEST_EVAL_CASE", None)
    print(
        "Running all 6 cases: thinking enabled, max_tokens=16384, context=32768, timeout=120s",
        flush=True,
    )
    await common.evaluate(
        args.test_binary.resolve(),
        ARM,
        output,
        args.work.resolve(),
        args.python.resolve(),
    )
    observations = common.rows(output / f"{ARM}.jsonl")
    prompt_hashes = {}
    complete_times = []
    for row in observations:
        request_file = output / ARM / row["case"] / "request.json"
        request = json.loads(request_file.read_text(encoding="utf-8"))
        old = json.loads(
            (reference / "deepseek" / row["case"] / "request.json").read_text(
                encoding="utf-8"
            )
        )
        assert request["messages"] == old["messages"]
        assert request["max_tokens"] == 16384
        prompt_hashes[row["case"]] = hashlib.sha256(
            json.dumps(request["messages"], sort_keys=True, ensure_ascii=False).encode()
        ).hexdigest()
        response = output / ARM / row["case"] / "response.json"
        if response.exists():
            complete_times.append(row["generation_ms"])
    summary = {
        "arm": ARM,
        "observations": len(observations),
        "http_responses": len(complete_times),
        "accepted": sum(r["accepted"] for r in observations),
        "baseline_pass": sum(
            bool((r["baseline"] or {}).get("success")) for r in observations
        ),
        "mutants_killed": sum(m["killed"] for r in observations for m in r["mutants"]),
        "mutants_exercised": sum(len(r["mutants"]) for r in observations),
        "mutants_available": 18,
        "response_median_ms": statistics.median(complete_times)
        if complete_times
        else None,
        "response_total_ms": sum(complete_times),
        "prompt_hashes": prompt_hashes,
        "daily_config_unchanged": sha(config_path) == config_before,
    }
    common.write_json(output / "summary.json", summary)
    assert summary["daily_config_unchanged"]
    print(json.dumps(summary, ensure_ascii=False, indent=2), flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--reference",
        type=Path,
        default=ROOT / "docs/benchmarks/2026-11-15-ai-test-generation",
    )
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
