#!/usr/bin/env python3
"""Run the actual Rust generation pipeline against controlled local model arms.

Uses synthetic fixtures only; no repository writes or online model requests.
Build the ignored Rust evaluation tests first and pass their test executable.
"""

from __future__ import annotations

import argparse
import asyncio
import contextlib
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time
from typing import AsyncIterator, Any
import urllib.error
import urllib.request


ROOT = Path(__file__).resolve().parents[1]
NO_WINDOW = 0x08000000 if os.name == "nt" else 0


def endpoint_port() -> int:
    """Reserve a unique loopback port for the owned server."""
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return int(listener.getsockname()[1])


def readiness(url: str, alias: str) -> bool:
    """Read model identity directly, bypassing all proxies."""
    try:
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        with opener.open(url + "/v1/models", timeout=2) as response:
            value = json.load(response)
        return any(item.get("id") == alias for item in value.get("data", []))
    except (OSError, ValueError, urllib.error.URLError):
        return False


async def stop(process: asyncio.subprocess.Process) -> None:
    """Stop and reap only a child process owned by this evaluation."""
    if process.returncode is None:
        process.terminate()
        try:
            await asyncio.wait_for(process.wait(), timeout=15)
        except TimeoutError:
            process.kill()
            await process.wait()


@contextlib.asynccontextmanager
async def model_server(
    executable: Path, model: Path, alias: str, output: Path
) -> AsyncIterator[tuple[str, float]]:
    """Load one model at a time and guarantee process exit before the next."""
    port = endpoint_port()
    address = f"http://127.0.0.1:{port}"
    command = [
        str(executable),
        "--model",
        str(model),
        "--alias",
        alias,
        "--host",
        "127.0.0.1",
        "--port",
        str(port),
        "--ctx-size",
        "16384",
        "--n-gpu-layers",
        "99",
        "--parallel",
        "1",
        "--flash-attn",
        "on",
        "--cache-type-k",
        "f16",
        "--cache-type-v",
        "f16",
        "--jinja",
        "--reasoning-format",
        "deepseek",
    ]
    started = time.monotonic()
    with output.open("w", encoding="utf-8") as log:
        process = await asyncio.create_subprocess_exec(
            *command,
            stdin=subprocess.DEVNULL,
            stdout=log,
            stderr=subprocess.STDOUT,
            creationflags=NO_WINDOW,
        )
        try:
            while not await asyncio.to_thread(readiness, address, alias):
                if process.returncode is not None:
                    raise RuntimeError(
                        f"Server exited {process.returncode}; inspect {output}"
                    )
                if time.monotonic() - started > 180:
                    raise TimeoutError(f"Model loading timed out; inspect {output}")
                await asyncio.sleep(0.25)
            yield address, (time.monotonic() - started) * 1000
        finally:
            await stop(process)


async def evaluate(
    binary: Path,
    address: str,
    alias: str,
    arm: str,
    thinking: bool,
    feature: str,
    output: Path,
    case: str | None = None,
) -> None:
    """Run production Rust prompts, parsing and validation; stream progress."""
    target = output / f"{arm}-{feature}.jsonl"
    if target.exists():
        raise FileExistsError(f"Refusing to mix old and new observations: {target}")
    environment = os.environ.copy()
    environment.update(
        {
            "CARGO_MANIFEST_DIR": str(ROOT / "helix-term"),
            "HELIX_RUNTIME": str(ROOT / "runtime"),
            "HELIX_AI_EVAL_ENDPOINT": address + "/v1/chat/completions",
            "HELIX_AI_EVAL_MODEL": alias,
            "HELIX_AI_EVAL_THINKING": str(thinking).lower(),
            "HELIX_AI_EVAL_OUTPUT": str(target.resolve()),
        }
    )
    if case:
        environment["HELIX_AI_EVAL_CASE"] = case
    test = {
        "doc": "handlers::ai_doc::client::evaluation::ai_doc_quality_evaluation",
        "commit": "handlers::ai_commit::evaluation::ai_commit_quality_evaluation",
    }[feature]
    with (output / f"{arm}-{feature}.test.log").open("w", encoding="utf-8") as log:
        process = await asyncio.create_subprocess_exec(
            str(binary),
            "--ignored",
            "--exact",
            test,
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
                if target.exists():
                    rows = [
                        json.loads(line)
                        for line in target.read_text(encoding="utf-8").splitlines()
                        if line.strip()
                    ]
                    for row in rows[reported:]:
                        print(
                            f"{arm} {feature} {row['id']}: {'ok' if row['error'] is None else row['error']} ({row['elapsed_ms']}ms)",
                            flush=True,
                        )
                    reported = len(rows)
                if time.monotonic() - started > 1800:
                    raise TimeoutError(f"Evaluation timed out: {arm}/{feature}")
                await asyncio.sleep(1)
            if process.returncode != 0:
                raise RuntimeError(
                    f"Rust evaluation failed: {arm}/{feature}, exit {process.returncode}"
                )
            expected = 1 if case else 6
            if (
                not target.exists()
                or len(target.read_text(encoding="utf-8").splitlines()) != expected
            ):
                raise RuntimeError(f"Expected {expected} observations: {target}")
        finally:
            await stop(process)


def default_paths() -> tuple[Path, Path, Path]:
    """Use the user's installed Junie model/runtime directory."""
    home = (
        Path(os.environ.get("USERPROFILE", str(Path.home())))
        / ".local/share/junie-local"
    )
    return (
        home / "versions/0.3.5/runtime/llama.cpp-b11081/llama-server.exe",
        home / "models/Qwen3.5-9B-Q6_K.gguf",
        home / "models/Qwen3.8-27B-UD-Q5_K_M.gguf",
    )


def ensure_idle() -> None:
    """Reject external model processes instead of interrupting them."""
    if os.name == "nt":
        result = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq llama-server.exe", "/FO", "CSV", "/NH"],
            capture_output=True,
            creationflags=NO_WINDOW,
            check=True,
        )
        if b'"llama-server.exe"' in result.stdout.lower():
            raise RuntimeError(
                "An external llama-server is running; close its owning editor before evaluation"
            )


async def run(args: argparse.Namespace) -> None:
    """Run three arms sequentially under the same production output budgets."""
    for path in (args.server, args.model_9b, args.model_27b, args.test_binary):
        if not path.is_file():
            raise FileNotFoundError(path)
    ensure_idle()
    args.output.mkdir(parents=True, exist_ok=True)
    manifest: dict[str, Any] = {
        "context_size": 16384,
        "final_token_allowance": 1024,
        "thinking_budget": 2048,
        "thinking_effort": "medium",
        "request_timeout_ms": 120000,
        "temperature": 0.0,
        "cases_per_feature": 1 if args.case else 6,
        "repetitions": 1,
        "arms": ["9b-direct", "27b-direct", "27b-thinking"]
        if args.arm == "all"
        else [args.arm],
        "sources": {},
        "load_ms": {},
    }
    for source in (
        "helix-term/src/handlers/ai_doc/client/evaluation.rs",
        "helix-term/src/handlers/ai_commit/evaluation.rs",
    ):
        manifest["sources"][source] = hashlib.sha256(
            (ROOT / source).read_bytes()
        ).hexdigest()
    manifest["diagnostic_case"] = args.case
    features = ("doc", "commit") if args.feature == "all" else (args.feature,)
    for name, model, arms in (
        ("9b", args.model_9b, (("9b-direct", False),)),
        ("27b", args.model_27b, (("27b-direct", False), ("27b-thinking", True))),
    ):
        selected_arms = [
            (arm, thinking) for arm, thinking in arms if args.arm in ("all", arm)
        ]
        if not selected_arms:
            continue
        async with model_server(
            args.server, model, "eval-" + name, args.output / f"{name}.server.log"
        ) as (address, load_ms):
            manifest["load_ms"][name] = load_ms
            for arm, thinking in selected_arms:
                for feature in features:
                    await evaluate(
                        args.test_binary,
                        address,
                        "eval-" + name,
                        arm,
                        thinking,
                        feature,
                        args.output,
                        args.case,
                    )
    (args.output / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    print(f"Completed: {args.output.resolve()}", flush=True)


def main() -> None:
    """Parse paths and execute the bounded, reproducible comparison."""
    server, model_9b, model_27b = default_paths()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--test-binary", type=Path, required=True)
    parser.add_argument("--server", type=Path, default=server)
    parser.add_argument("--model-9b", type=Path, default=model_9b)
    parser.add_argument("--model-27b", type=Path, default=model_27b)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--arm",
        choices=("all", "9b-direct", "27b-direct", "27b-thinking"),
        default="all",
    )
    parser.add_argument("--feature", choices=("all", "doc", "commit"), default="all")
    parser.add_argument(
        "--case",
        help="single-case diagnostic rerun; keep separate from primary observations",
    )
    args = parser.parse_args()
    if args.case and args.feature == "all":
        parser.error("--case requires --feature doc or commit")
    asyncio.run(run(args))


if __name__ == "__main__":
    main()
