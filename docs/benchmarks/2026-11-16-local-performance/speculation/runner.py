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
    for count in (1, 2, 3, 4, 5):
        ARMS[f"mtp{count}-q8-{model}"] = (
            model,
            ["-ngl", "99", "-ctk", "q8_0", "-ctv", "q8_0",
             "--spec-type", "draft-mtp", "--spec-draft-n-max", str(count)],
        )
    ARMS[f"ngram-q8-{model}"] = (
        model, ["-ngl", "99", "-ctk", "q8_0", "-ctv", "q8_0",
                "--spec-type", "ngram-simple"],
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


def speed_payload(long: bool, tokens: int) -> dict[str, Any]:
    fixture = "\n".join(
        f"def clamp_{n}(value: int, lower: int, upper: int) -> int:\n"
        f"    # Case {n}: retain values inside the inclusive interval.\n"
        "    return max(lower, min(upper, value))\n"
        for n in range(400 if long else 8)
    )
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


def evaluate(base: str, output: Path, workloads: list[str], repeats: int, tokens: int) -> list[dict[str, Any]]:
    observations = []
    for workload in workloads:
        for repetition in range(repeats):
            case = output / f"{workload}-{repetition}"
            case.mkdir()
            payload = quality_payload() if workload == "quality" else speed_payload(workload == "prefill", tokens)
            write_json(case / "request.json", payload)
            started = time.perf_counter()
            response = request(base, "/v1/chat/completions" if workload == "quality" else "/completion", payload)
            elapsed = time.perf_counter() - started
            write_json(case / "response.json", response)
            timing = response.get("timings", {})
            row = {"workload": workload, "repetition": repetition, "seconds": elapsed,
                   "timings": timing, "gpu_after": gpu()}
            if workload == "quality":
                row["quality"] = validate_quality(response)
                row["usage"] = response.get("usage")
            observations.append(row)
            write_json(case / "result.json", row)
            print(json.dumps(row, ensure_ascii=False), flush=True)
    return observations


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--arms", default="baseline-27b,full-q8-27b,mtp3-q8-27b")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--server", type=Path, default=SERVER)
    parser.add_argument("--context", type=int)
    parser.add_argument("--extra", nargs=argparse.REMAINDER, default=[])
    parser.add_argument("--workloads", default="decode,quality")
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--tokens", type=int, default=256)
    args = parser.parse_args()
    arms = args.arms.split(",")
    workloads = args.workloads.split(",")
    if not all(arm in ARMS for arm in arms) or not all(w in ("decode", "prefill", "quality") for w in workloads):
        parser.error("Unknown arm or workload")
    if args.repeats < 1 or args.tokens < 1:
        parser.error("repeats and tokens must be positive")
    ensure_idle()
    args.output.mkdir(parents=True, exist_ok=False)
    manifest = {"arguments": vars(args) | {"output": str(args.output), "server": str(args.server)},
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
                rows = evaluate(base, output, workloads, args.repeats, args.tokens)
            speeds = [r["timings"]["predicted_per_second"] for r in rows
                      if r["workload"] == "decode"]
            manifest["results"][arm] = {"metadata": metadata, "observations": rows,
                                        "decode_median": statistics.median(speeds) if speeds else None}
        except Exception as error:
            manifest["results"][arm] = {"error": str(error)}
            print(f"{arm} failed: {error}", flush=True)
        write_json(args.output / "manifest.json", manifest)


if __name__ == "__main__":
    main()
