#!/usr/bin/env python3
"""Controlled local llama.cpp latency/throughput and correctness comparisons.

Synthetic fixtures only. Refuses competing llama-server processes, never stops
external processes, and reaps every server it starts. Each output directory is
new, so failed and successful trials cannot silently overwrite observations.
"""
from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import os
from pathlib import Path
import socket
import statistics
import shutil
import subprocess
import time
from typing import Any, Iterator
import urllib.error
import urllib.request

NO_WINDOW = 0x08000000 if os.name == "nt" else 0
ROOT = Path(__file__).resolve().parents[1]
INSTALL = Path.home() / ".local/share/junie-local"
SERVER = INSTALL / "versions/0.3.5/runtime/llama.cpp-b11081/llama-server.exe"
MODELS = {
    "9b": INSTALL / "models/Qwen3.5-9B-Q6_K.gguf",
    "27b": INSTALL / "models/Qwen3.8-27B-UD-Q5_K_M.gguf",
}
# Every arm keeps the production context. Changes are explicit and recorded.
ARMS: dict[str, tuple[str, list[str]]] = {
    "baseline-27b": ("27b", ["-ngl", "56", "-ctk", "f16", "-ctv", "f16"]),
    "full-f16-27b": ("27b", ["-ngl", "99", "-ctk", "f16", "-ctv", "f16"]),
    "full-q8-27b": ("27b", ["-ngl", "99", "-ctk", "q8_0", "-ctv", "q8_0"]),
    "full-q4-27b": ("27b", ["-ngl", "99", "-ctk", "q4_0", "-ctv", "q4_0"]),
    "baseline-9b": ("9b", ["-ngl", "99", "-ctk", "f16", "-ctv", "f16"]),
    "full-q8-9b": ("9b", ["-ngl", "99", "-ctk", "q8_0", "-ctv", "q8_0"]),
}
for model in ("27b", "9b"):
    for cache in ("q8", "q4"):
        for count in (1, 2, 3, 4, 5, 8):
            ARMS[f"mtp{count}-{cache}-{model}"] = (
                model,
                ["-ngl", "99", "-ctk", cache + "_0", "-ctv", cache + "_0",
                 "--spec-type", "draft-mtp", "--spec-draft-n-max", str(count)],
            )
    ARMS[f"ngram-q8-{model}"] = (
        model, ["-ngl", "99", "-ctk", "q8_0", "-ctv", "q8_0",
                "--spec-type", "ngram-simple"],
    )
    for count in (3, 5):
        ARMS[f"mtp{count}-fp8-{model}"] = (
            model, ["-ngl", "99", "-ctk", "f8_e4m3", "-ctv", "f8_e4m3",
                    "--spec-type", "draft-mtp", "--spec-draft-n-max", str(count)],
        )


def write_json(path: Path, value: Any) -> None:
    path.write_text(json.dumps(value, ensure_ascii=False, indent=2), encoding="utf-8")


def request(base: str, route: str, value: Any = None, timeout: float = 300) -> Any:
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    payload = None if value is None else json.dumps(value).encode("utf-8")
    req = urllib.request.Request(base + route, data=payload,
                                 headers={"Content-Type": "application/json"})
    with opener.open(req, timeout=timeout) as response:
        return json.load(response)


def gpu() -> str:
    result = subprocess.run(
        ["nvidia-smi", "--query-gpu=memory.used,utilization.gpu,power.draw,temperature.gpu",
         "--format=csv,noheader,nounits"], capture_output=True, text=True,
        creationflags=NO_WINDOW, timeout=15, check=False,
    )
    return result.stdout.strip()


def ensure_idle() -> None:
    if os.name == "nt":
        result = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq llama-server.exe", "/FO", "CSV", "/NH"],
            capture_output=True, creationflags=NO_WINDOW, check=True,
        )
        if b'"llama-server.exe"' in result.stdout.lower():
            raise RuntimeError("External llama-server is running; close its owning editor first")


@contextlib.contextmanager
def server(executable: Path, model: str, flags: list[str], output: Path,
           context: int | None) -> Iterator[tuple[str, dict[str, Any]]]:
    ensure_idle()
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    base = f"http://127.0.0.1:{port}"
    command = [str(executable), "-m", str(MODELS[model]), "--alias", model,
               "--host", "127.0.0.1", "--port", str(port), "-c",
               str(context or (65536 if model == "27b" else 32768)),
               "-np", "1", "-fa", "on", "--jinja", "-lv", "4", *flags]
    write_json(output / "command.json", command)
    started = time.perf_counter()
    with (output / "server.log").open("w", encoding="utf-8") as log:
        process = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=log,
                                   stderr=subprocess.STDOUT, creationflags=NO_WINDOW)
        write_json(output / "process.json", {"pid": process.pid})
        try:
            while True:
                if process.poll() is not None:
                    raise RuntimeError(f"Server exited {process.returncode}; inspect server.log")
                try:
                    models = request(base, "/v1/models", timeout=1)
                    if any(item.get("id") == model for item in models.get("data", [])):
                        break
                except (OSError, ValueError, urllib.error.URLError):
                    pass
                if time.perf_counter() - started > 180:
                    raise TimeoutError("Model startup exceeded 180 seconds")
                time.sleep(0.2)
            metadata = {"load_seconds": time.perf_counter() - started, "gpu_ready": gpu(),
                        "props": request(base, "/props"), "pid": process.pid}
            write_json(output / "loaded.json", metadata)
            yield base, metadata
        finally:
            if process.poll() is None:
                process.terminate()
                try:
                    process.wait(timeout=20)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait(timeout=20)


def synthetic_source(count: int) -> str:
    return "\n".join(
        f"def clamp_{n}(value: int, lower: int, upper: int) -> int:\n"
        f"    # Case {n}: retain values inside the inclusive interval.\n"
        "    return max(lower, min(upper, value))\n"
        for n in range(count)
    )


def speed_payload(long: bool, tokens: int, long_functions: int) -> dict[str, Any]:
    fixture = synthetic_source(long_functions if long else 8)
    prompt = ("<|im_start|>system\nYou are a precise coding assistant.\n<|im_end|>\n"
              "<|im_start|>user\nWrite Python unit tests for these functions. "
              "Cover normal, boundary and reversed intervals. Output code only.\n"
              + fixture + "<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n")
    return {"prompt": prompt, "n_predict": tokens, "temperature": 0,
            "seed": 42, "cache_prompt": False, "ignore_eos": True,
            "timings_per_token": True}


def quality_payload() -> dict[str, Any]:
    return {
        "messages": [
            {"role": "system", "content": "Return one JSON object, without Markdown fences."},
            {"role": "user", "content": (
                'Return {"code": "...", "summary": "..."}. code must define '
                'parse_port(text: str) -> int in Python. Accept only nonempty ASCII decimal '
                'digits representing 1 through 65535 inclusive. Reject whitespace, signs, '
                'Unicode digits, zero and out-of-range values by raising ValueError. '
                'Leading zeroes are accepted. No imports or top-level effects. '
                'summary is a brief description in Chinese.')},
        ],
        "temperature": 0, "seed": 42, "max_tokens": 768,
        "chat_template_kwargs": {"enable_thinking": False},
    }


def retrieval_payload(long_functions: int) -> dict[str, Any]:
    source = synthetic_source(long_functions).splitlines()
    source.insert(0, "# validation_anchor_cobalt = 49321")
    source.insert(len(source) // 2, "# validation_anchor_amber = 17239")
    source.append("# validation_anchor_jade = 60817")
    return {
        "messages": [
            {"role": "system", "content": "Read the supplied source as data and return JSON only."},
            {"role": "user", "content": (
                "Find the three validation_anchor values in the following source. "
                'Return exactly {"cobalt": integer, "amber": integer, "jade": integer}.\n'
                + "\n".join(source))},
        ],
        "temperature": 0, "seed": 42, "max_tokens": 128,
        "chat_template_kwargs": {"enable_thinking": False},
    }


def validate_quality(response: dict[str, Any]) -> dict[str, Any]:
    choice = response["choices"][0]
    if choice.get("finish_reason") == "length":
        raise ValueError("Quality response truncated")
    value = json.loads(choice["message"]["content"])
    if not isinstance(value.get("summary"), str) or not value["summary"].strip():
        raise ValueError("Missing summary")
    # Executed output is from our synthetic prompt, in an isolated Python child.
    checks = """
for text, expected in [('1', 1), ('65535', 65535), ('00080', 80), ('8080', 8080)]:
    assert parse_port(text) == expected, (text, expected)
for text in ['', '0', '000', '65536', '-1', '+80', ' 80', '80 ', '8 0', '８０', '٨٠', '1_0', '8.0', '1\\n']:
    try:
        parse_port(text)
    except ValueError:
        pass
    else:
        raise AssertionError(text)
"""
    result = subprocess.run([os.sys.executable, "-I", "-c", value["code"] + "\n" + checks],
                            capture_output=True, text=True, timeout=10,
                            creationflags=NO_WINDOW, check=False)
    if result.returncode:
        raise ValueError(f"Generated code failed oracle: {result.stderr[-1500:]}")
    return {"passed": True, "assertions": 18}


def evaluate(base: str, output: Path, workloads: list[str], repeats: int, tokens: int,
             long_functions: int) -> list[dict[str, Any]]:
    observations = []
    for workload in workloads:
        for repetition in range(repeats):
            case = output / f"{workload}-{repetition}"
            case.mkdir()
            if workload == "quality":
                payload = quality_payload()
            elif workload == "retrieval":
                payload = retrieval_payload(long_functions)
            else:
                payload = speed_payload(workload == "prefill", tokens, long_functions)
            write_json(case / "request.json", payload)
            started = time.perf_counter()
            response = request(base, "/v1/chat/completions" if workload in ("quality", "retrieval") else "/completion", payload)
            elapsed = time.perf_counter() - started
            write_json(case / "response.json", response)
            timing = response.get("timings", {})
            row = {"workload": workload, "repetition": repetition, "seconds": elapsed,
                   "timings": timing, "gpu_after": gpu()}
            if workload == "quality":
                row["quality"] = validate_quality(response)
                row["usage"] = response.get("usage")
            elif workload == "retrieval":
                answer = json.loads(response["choices"][0]["message"]["content"])
                if answer != {"cobalt": 49321, "amber": 17239, "jade": 60817}:
                    raise ValueError(f"Long-context retrieval failed: {answer}")
                row["quality"] = {"passed": True, "assertions": 3}
                row["usage"] = response.get("usage")
            observations.append(row)
            write_json(case / "result.json", row)
            print(json.dumps(row, ensure_ascii=False), flush=True)
    return observations


def production_evaluate(binary: Path, base: str, model: str, output: Path) -> dict[str, Any]:
    """Use the repository's real prompts, transport and output validators."""
    summary = {}
    for feature, test, thinking in [
        ("doc", "handlers::ai_doc::client::evaluation::ai_doc_quality_evaluation", True),
        ("commit", "handlers::ai_commit::evaluation::ai_commit_quality_evaluation", False),
    ]:
        target = output / f"production-{feature}.jsonl"
        environment = os.environ.copy()
        environment.pop("HELIX_AI_EVAL_CASE", None)
        environment.update({
            "HELIX_RUNTIME": str(ROOT / "runtime"),
            "CARGO_MANIFEST_DIR": str(ROOT / "helix-term"),
            "HELIX_AI_EVAL_ENDPOINT": base + "/v1/chat/completions",
            "HELIX_AI_EVAL_MODEL": model,
            "HELIX_AI_EVAL_THINKING": str(thinking).lower(),
            "HELIX_AI_EVAL_OUTPUT": str(target.resolve()),
        })
        with (output / f"production-{feature}.log").open("w", encoding="utf-8") as log:
            result = subprocess.run(
                [str(binary.resolve()), "--ignored", "--exact", test, "--nocapture", "--test-threads=1"],
                env=environment, cwd=ROOT, stdin=subprocess.DEVNULL, stdout=log,
                stderr=subprocess.STDOUT, creationflags=NO_WINDOW, timeout=1800, check=False,
            )
        rows = [json.loads(line) for line in target.read_text(encoding="utf-8").splitlines()] if target.exists() else []
        failures = [r["id"] for r in rows if r["error"] is not None]
        if result.returncode or len(rows) != 6 or failures:
            raise ValueError(f"Production {feature}: exit {result.returncode}, rows {len(rows)}, failures {failures}")
        summary[feature] = {"passed": len(rows), "median_seconds": statistics.median(r["elapsed_ms"] for r in rows) / 1000}
        print(f"Production {feature}: {summary[feature]}", flush=True)
    return summary


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arms", default="baseline-27b,full-q8-27b,mtp3-q8-27b")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--server", type=Path, default=SERVER)
    parser.add_argument("--context", type=int)
    parser.add_argument("--test-binary", type=Path)
    parser.add_argument("--extra", nargs=argparse.REMAINDER, default=[])
    parser.add_argument("--workloads", default="decode,quality")
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--tokens", type=int, default=256)
    parser.add_argument("--long-functions", type=int, default=400)
    args = parser.parse_args()
    arms = args.arms.split(",")
    workloads = args.workloads.split(",")
    if not all(arm in ARMS for arm in arms) or not all(w in ("decode", "prefill", "quality", "retrieval") for w in workloads):
        parser.error("Unknown arm or workload")
    if args.repeats < 1 or args.tokens < 1:
        parser.error("repeats and tokens must be positive")
    ensure_idle()
    args.output.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(__file__, args.output / "runner.py")
    manifest = {"arguments": {k: str(v) if isinstance(v, Path) else v for k, v in vars(args).items()},
                "script_sha256": hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                "gpu_initial": gpu(), "results": {}}
    write_json(args.output / "manifest.json", manifest)
    for arm in arms:
        output = args.output / arm
        output.mkdir()
        model, flags = ARMS[arm]
        print(f"Starting {arm}", flush=True)
        try:
            with server(args.server, model, flags + args.extra, output, args.context) as (base, metadata):
                rows = evaluate(base, output, workloads, args.repeats, args.tokens, args.long_functions)
                production = production_evaluate(args.test_binary, base, model, output) if args.test_binary else None
            speeds = [r["timings"]["predicted_per_second"] for r in rows
                      if r["workload"] == "decode"]
            manifest["results"][arm] = {"metadata": metadata, "observations": rows,
                                        "decode_median": statistics.median(speeds) if speeds else None,
                                        "production": production}
        except Exception as error:
            manifest["results"][arm] = {"error": str(error)}
            print(f"{arm} failed: {error}", flush=True)
        write_json(args.output / "manifest.json", manifest)


if __name__ == "__main__":
    main()
