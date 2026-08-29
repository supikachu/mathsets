#!/usr/bin/env python3
"""L2 建议草案：读冻结集 L0/L1，只写 advice/ 目录。

允许三类输出：删/降权规则候选、硬切片→候选跨度的证据、slice 夹具草案路径。
Prompt 建议拆成任务定义 / Schema / 硬约束三层 diff，不覆盖 docs/rules-prompts.md。
不替换现网 split_question_chunks，不生成 if-elif 补丁。

用法::

    python scripts/bench_eval_advise.py --dir bench/evalset/v0
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_DIR = REPO_ROOT / "bench" / "evalset" / "v0"


def _stdio() -> None:
    for stream in (sys.stdout, sys.stderr):
        reconf = getattr(stream, "reconfigure", None)
        if reconf is not None:
            try:
                reconf(encoding="utf-8")
            except Exception:
                pass


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.is_file():
        return rows
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(row, dict):
            rows.append(row)
    return rows


def collect_samples(root: Path) -> list[dict[str, Any]]:
    errors_dir = root / "errors"
    rows: list[dict[str, Any]] = []
    if errors_dir.is_dir():
        for p in sorted(errors_dir.glob("*.jsonl")):
            rows.extend(load_jsonl(p))
    for p in sorted(root.glob("*/*.jsonl")):
        if p.parent.name == "errors":
            continue
        if p.name.endswith(".jsonl") and "error" in p.name:
            rows.extend(load_jsonl(p))
    return rows


def write_advice(root: Path, samples: list[dict[str, Any]], report: dict[str, Any] | None) -> None:
    advice = root / "advice"
    advice.mkdir(parents=True, exist_ok=True)
    tag_hist: Counter[str] = Counter()
    shunt_hist: Counter[str] = Counter()
    bucket_hist: Counter[str] = Counter()
    by_tag: dict[str, list[str]] = defaultdict(list)
    both_fail_n = 0
    for s in samples:
        shunt_hist[str(s.get("shunt") or "ok")] += 1
        if s.get("shunt") == "both_fail":
            both_fail_n += 1
        for err in s.get("errors") or []:
            if not isinstance(err, dict):
                continue
            tag = str(err.get("tag") or "")
            bucket = str(err.get("rule_bucket") or "")
            if tag:
                tag_hist[tag] += 1
                qid = f"{s.get('paper_id')}:{s.get('question_no')}"
                if qid not in by_tag[tag]:
                    by_tag[tag].append(qid)
            if bucket:
                bucket_hist[bucket] += 1

    stamped = datetime.now(timezone.utc).isoformat()
    n = len(samples) or 1

    rules_lines = [
        "# 规则删除/降权候选（草案）",
        "",
        f"生成时间：{stamped}",
        f"样本数：{len(samples)}",
        "",
        "本文件不是补丁。禁止直接改 `src/ai/slice`。人工确认后再开独立任务。",
        "",
        "| 标签 | 次数 | 失败率（相对 L1 样本） | 题号清单 |",
        "|------|------|------------------------|----------|",
    ]
    for tag, c in tag_hist.most_common():
        qs = "、".join(by_tag[tag][:20])
        extra = f" 等{len(by_tag[tag])}题" if len(by_tag[tag]) > 20 else ""
        rules_lines.append(f"| `{tag}` | {c} | {c / n:.2%} | {qs}{extra} |")
    rules_lines += [
        "",
        "## 建议怎么用",
        "",
        "1. 只允许「删除或降权一条**现有**规则」，并附冻结集失败率。",
        "2. 不要生成 if-elif 补丁。",
        "3. 准入：目标桶下降；其它桶失败题数不得 +2 或失败率 +3pp（需求分析 §7.1）。",
        "",
    ]
    (advice / "delete_or_demote_rules.md").write_text("\n".join(rules_lines), encoding="utf-8")

    span_lines = [
        "# 硬切片 → 候选跨度（架构证据，草案）",
        "",
        f"生成时间：{stamped}",
        "",
        "评测脚本 **不得** 替换现网 `split_question_chunks`。",
        "",
        f"- L1 样本 {len(samples)}；切题相关 `question_boundary_error` {tag_hist.get('question_boundary_error', 0)}；",
        f"  `question_merge` {tag_hist.get('question_merge', 0)}；`question_split` {tag_hist.get('question_split', 0)}。",
        f"- 分流 both_fail {shunt_hist.get('both_fail', 0)}，slice_fixture {shunt_hist.get('slice_fixture', 0)}。",
        "",
        "若切题桶在冻结集上持续失败，证据支持「算法只标可能是题/答案/解析区，结构交给 LLM」。",
        "这是产品架构变更，另开任务，不要在本目录合入代码。",
        "",
    ]
    (advice / "candidate_span_evidence.md").write_text("\n".join(span_lines), encoding="utf-8")

    fixture_lines = [
        "# slice 夹具草案（先红后绿）",
        "",
        f"生成时间：{stamped}",
        "",
        "夹具合入 `src/ai/slice` 须另开 PR。下列只是候选路径列表。",
        "",
    ]
    fixture_qs = by_tag.get("question_boundary_error", []) + by_tag.get("question_merge", []) + by_tag.get(
        "question_split", []
    )
    seen: set[str] = set()
    for qid in fixture_qs:
        if qid in seen:
            continue
        seen.add(qid)
        paper, _, no = qid.partition(":")
        fixture_lines.append(f"- `src/ai/slice` 夹具候选：`{paper}` 题 `{no}`")
    if not seen:
        fixture_lines.append("（本轮没有切题类标签；先看 L0 `cut` / `slice_fixture`。）")
    fixture_lines += ["", "先写失败测试，再改规则，回归同一 `markdown_sha256`。", ""]
    (advice / "slice_fixture_candidates.md").write_text("\n".join(fixture_lines), encoding="utf-8")

    prompt_ok = both_fail_n > 0 or shunt_hist.get("export_missed_prompt", 0) > 0
    prompt_lines = [
        "# Prompt 三层 diff（草案，不覆盖 rules-prompts.md）",
        "",
        f"生成时间：{stamped}",
        "",
        "全自动运行时走 `STAGE2_PATCH_PROMPT`，站外走 `docs/rules-prompts.md`。",
        "不要把 PATCH 失败算到 FULL 头上。变更须与 `CORE_PARSE_RULES` 测试锁定同步。",
        "",
    ]
    if not prompt_ok:
        prompt_lines += [
            "**本轮没有 both_fail / 站外系统性违反已有条文的证据，不出 Prompt 条文。**",
            "",
        ]
    else:
        prompt_lines += [
            f"both_fail 样本 {both_fail_n}；export_missed_prompt {shunt_hist.get('export_missed_prompt', 0)}。",
            "",
            "## 1. 任务定义",
            "",
            "- （人工填写）是否仍禁止做题、是否仍以 OCR 为事实源。",
            "",
            "## 2. 结构规范（Schema）",
            "",
            "- （人工填写）`parts` / `correct_answer.kind` / 选择题 options。",
            "",
            "## 3. 硬约束",
            "",
            "- （人工填写）仅当两边相对 Markdown 同错，或站外违反已有条文。",
            "",
            "裁判脚本不得直接覆盖 `docs/rules-prompts.md`。",
            "",
        ]
    (advice / "prompt_three_layer_diff.md").write_text("\n".join(prompt_lines), encoding="utf-8")

    summary = {
        "schema_version": "advice.v1",
        "generated_at": stamped,
        "sample_count": len(samples),
        "tag_histogram": dict(tag_hist),
        "shunt_histogram": dict(shunt_hist),
        "bucket_histogram": dict(bucket_hist),
        "files": [
            "delete_or_demote_rules.md",
            "candidate_span_evidence.md",
            "slice_fixture_candidates.md",
            "prompt_three_layer_diff.md",
        ],
        "report_question_count": (report or {}).get("question_count"),
        "writes_source": False,
    }
    (advice / "summary.json").write_text(
        json.dumps(summary, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )


def main() -> int:
    _stdio()
    parser = argparse.ArgumentParser(description="L2 建议草案（只写 advice/，不改 slice/Prompt）")
    parser.add_argument("--dir", default=str(DEFAULT_DIR), help="冻结集或 dump 根目录")
    args = parser.parse_args()
    root = Path(args.dir)
    if not root.is_absolute():
        root = (REPO_ROOT / root).resolve()
    if not root.is_dir():
        raise SystemExit(f"{root} 不存在")
    samples = collect_samples(root)
    report = None
    latest = root / "report_latest.json"
    if latest.is_file():
        try:
            loaded = json.loads(latest.read_text(encoding="utf-8"))
            if isinstance(loaded, dict):
                report = loaded
        except (OSError, json.JSONDecodeError):
            report = None
    write_advice(root, samples, report)
    print(f"已写 {root / 'advice'}（{len(samples)} 条 L1 样本；未改 slice / 提示词）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
