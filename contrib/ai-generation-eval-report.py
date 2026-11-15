#!/usr/bin/env python3
"""Summarize raw observations using hash-bound, explicitly reviewed rubric grades."""

from __future__ import annotations

import argparse
import csv
from dataclasses import dataclass
import hashlib
import json
from pathlib import Path
import statistics
from typing import Any


@dataclass(frozen=True)
class Summary:
    """Feature-level metrics for one model arm."""

    arm: str
    feature: str
    count: int
    accepted: int
    perfect: int
    score: float
    median_seconds: float


def observations(directory: Path) -> list[dict[str, Any]]:
    """Validate each reviewed grade against the exact saved model observation."""
    grades = json.loads((directory / "grades.json").read_text(encoding="utf-8"))[
        "grades"
    ]
    result: list[dict[str, Any]] = []
    for path in sorted(directory.glob("*-*.jsonl")):
        feature = path.stem.rsplit("-", 1)[1]
        arm = path.stem.rsplit("-", 1)[0]
        for line in path.read_text(encoding="utf-8").splitlines():
            row = json.loads(line)
            grade = grades[arm + "/" + row["id"]]
            payload = json.dumps(
                row,
                ensure_ascii=False,
                sort_keys=True,
                separators=(",", ":"),
            ).encode()
            if hashlib.sha256(payload).hexdigest() != grade["observation_sha256"]:
                raise ValueError(
                    f"Grade belongs to a different observation: {arm}/{row['id']}"
                )
            criteria = grade["criteria"]
            if len(criteria) != 4 or any(type(value) is not bool for value in criteria):
                raise ValueError("Expected four binary rubric criteria")
            score = 25 * sum(criteria)
            if score != grade["score"] or (row["error"] is not None and score != 0):
                raise ValueError("Inconsistent reviewed score")
            result.append(
                {
                    "arm": arm,
                    "feature": feature,
                    "case": row["id"],
                    "accepted": row["error"] is None,
                    "score": score,
                    "elapsed_ms": row["elapsed_ms"],
                    "criteria": criteria,
                    "notes": grade["notes"],
                    "error": row["error"],
                }
            )
    return result


def summarize(rows: list[dict[str, Any]]) -> list[Summary]:
    """Compute small-sample means and medians without excluding failed tasks."""
    return [
        Summary(
            arm,
            feature,
            len(group),
            sum(row["accepted"] for row in group),
            sum(row["score"] == 100 for row in group),
            statistics.mean(row["score"] for row in group),
            statistics.median(row["elapsed_ms"] for row in group) / 1000,
        )
        for arm in ("9b-direct", "27b-direct", "27b-thinking")
        for feature in ("doc", "commit")
        if (
            group := [
                row for row in rows if row["arm"] == arm and row["feature"] == feature
            ]
        )
    ]


def write_report(directory: Path, rows: list[dict[str, Any]]) -> None:
    """Write transparent numerical summaries and a per-case grading CSV."""
    summaries = summarize(rows)
    with (directory / "per-case.csv").open(
        "w", encoding="utf-8-sig", newline=""
    ) as file:
        writer = csv.writer(file)
        writer.writerow(
            (
                "arm",
                "feature",
                "case",
                "accepted",
                "score",
                "elapsed_ms",
                "c1",
                "c2",
                "c3",
                "c4",
                "notes",
                "error",
            )
        )
        for row in rows:
            writer.writerow(
                (
                    row["arm"],
                    row["feature"],
                    row["case"],
                    row["accepted"],
                    row["score"],
                    row["elapsed_ms"],
                    *row["criteria"],
                    row["notes"],
                    row["error"],
                )
            )
    labels = {
        "9b-direct": "9B，不思考",
        "27b-direct": "27B，不思考",
        "27b-thinking": "27B，有限思考",
    }
    lines = [
        "# 本地模型对照实测",
        "",
        "硬件：RTX 4090（24GB），64GB 系统内存。对比的是实际安装的 Qwen3.5-9B Q6_K 与 Qwen3.8-27B Q5_K_M，包含模型代际和量化差异，不能把差异单独归因于参数量。",
        "",
        "每组6个注释任务、6个提交任务，共36个功能级观测；提交任务包含多次模型请求。每例运行一次。使用现有生产提示词、分批分析、格式化和校验逻辑，输入均为合成代码/diff。",
        "",
        "| 模型设置 | 功能 | 规则评分 /100 | 全项通过 | 可用输出 | 任务耗时中位数 |",
        "| --- | --- | ---: | ---: | ---: | ---: |",
    ]
    for item in summaries:
        lines.append(
            f"| {labels[item.arm]} | {'ai-doc' if item.feature == 'doc' else 'ai-commit'} | {item.score:.1f} | {item.perfect}/{item.count} | {item.accepted}/{item.count} | {item.median_seconds:.3f}s |"
        )
    lines += ["", "## 相对9B的变化", ""]
    for feature in ("doc", "commit"):
        base = next(
            item
            for item in summaries
            if item.arm == "9b-direct" and item.feature == feature
        )
        for item in summaries:
            if item.feature == feature and item.arm != "9b-direct":
                lines.append(
                    f"- {'ai-doc' if feature == 'doc' else 'ai-commit'}，{labels[item.arm]}：规则评分 {item.score - base.score:+.1f} 分；任务耗时中位数为9B的 {item.median_seconds / base.median_seconds:.2f} 倍。"
                )
    lines += [
        "",
        "## 结果解读",
        "",
        "评分增减与输出可用率需分别查看；思考模式是否值得启用，还应结合逐例内容和额外耗时判断。",
        "",
        "可用输出率由现有程序自动校验；内容评分是Codex助手按预先固定的四项二值规则逐例判读，不是独立专家盲评。参数/字段、核心行为、边界/范围、事实依据分别计25分；无法通过生产校验的结果记0分。",
        "",
        "样例少、每例只有一次，不是总体准确率、统计显著性结论，也不支持宣称所有项目都会提升相同比例。耗时包含测试进程中的解析/生成流程和失败任务，不含模型加载；测试程序采用未优化构建，不能当作发行版性能保证。27B有限思考在27B直答之后运行，存在缓存/顺序影响。",
        "",
        "## 可审计文件",
        "",
        "- `rubric.md`：运行前固定的评分规则。",
        "- `*-doc.jsonl`、`*-commit.jsonl`：每例输入、模型设置、原始功能输出/错误和耗时。",
        "- `grades.json`：四项逐例评分、扣分理由及绑定原始观测的哈希。",
        "- `per-case.csv`：逐例评分明细。",
        "- `manifest.json`：上下文、token预算、超时、载入时间和样例源代码哈希。",
        "- `diagnostic/`：失败例的额外诊断，不计入上述首轮成绩。",
        "",
        "复算汇总：`python contrib/ai-generation-eval-report.py --input docs/benchmarks/2026-11-14-local-generation`。重新评测应使用新目录，之后按同一rubric重新核查内容评分，不能套用旧grades。",
        "",
    ]
    failures = [row for row in rows if row["error"] is not None]
    lines += ["## 首轮失败记录", ""]
    for row in failures:
        lines.append(f"- {row['arm']}/{row['case']}：{row['error']}。")
    diagnostic = directory / "diagnostic/27b-thinking-commit.jsonl"
    if diagnostic.exists():
        for line in diagnostic.read_text(encoding="utf-8").splitlines():
            row = json.loads(line)
            if row["error"]:
                lines += ["", f"额外诊断（不计入成绩）：{row['id']}：{row['error']}。"]
    lines.append("")
    (directory / "report.md").write_text("\n".join(lines), encoding="utf-8")
    print("\n".join(lines[:15]))


def main() -> None:
    """Generate metrics only when all raw-output/grade hashes match."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--input", type=Path, required=True)
    args = parser.parse_args()
    write_report(args.input, observations(args.input))


if __name__ == "__main__":
    main()
