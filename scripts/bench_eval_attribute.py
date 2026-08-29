#!/usr/bin/env python3
"""L1 错因：把 L0 规则分失败题写成 error_sample.v1 jsonl。

不调用 LLM（formula_corruption / semantic_loss / hallucination 标 needs_fidelity_judge）。
不重打六桶，不输出改进 JSON，不写 src/ 或 docs/rules-prompts.md。

用法::

    python scripts/bench_eval_attribute.py
    python scripts/bench_eval_attribute.py --dir bench/evalset/v0
"""

from __future__ import annotations

import argparse
import json
import sys
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_EVAL = REPO_ROOT / "bench" / "eval"
JUDGE_DOC = "docs/全自动解析质量评估_裁判沟通词.md"
SCHEMA = "error_sample.v1"

BUCKET_TAGS = {
    "cut": "question_boundary_error",
    "choice_schema": "option_error",
    "answer": "answer_mapping_error",
    "parts": "sub_question_missing",
    "editorial": "analysis_mapping_error",
    "schema": "schema_error",
}
STAGE_OF = {
    "cut": "cut",
    "choice_schema": "llm",
    "answer": "llm",
    "parts": "cut",
    "editorial": "polish",
    "schema": "import",
}
FIDELITY_TAGS = ("formula_corruption", "semantic_loss", "hallucination")


def _stdio() -> None:
    for stream in (sys.stdout, sys.stderr):
        reconf = getattr(stream, "reconfigure", None)
        if reconf is not None:
            try:
                reconf(encoding="utf-8")
            except Exception:
                pass


def list_paper_dirs(root: Path) -> list[Path]:
    if (root / "paper.md").is_file():
        return [root]
    return [
        p
        for p in sorted(root.iterdir())
        if p.is_dir()
        and (p / "paper.md").is_file()
        and not p.name.startswith(("_", "."))
    ]


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def load_meta(paper_dir: Path) -> dict[str, Any]:
    meta_path = paper_dir / "meta.json"
    if not meta_path.is_file():
        return {}
    try:
        data = load_json(meta_path)
    except (OSError, json.JSONDecodeError):
        return {}
    return data if isinstance(data, dict) else {}


def parsed_by_no(payload: Any) -> dict[str, dict[str, Any]]:
    if isinstance(payload, dict):
        items = payload.get("questions")
    elif isinstance(payload, list):
        items = payload
    else:
        items = None
    if not isinstance(items, list):
        return {}
    out: dict[str, dict[str, Any]] = {}
    for i, item in enumerate(items):
        if not isinstance(item, dict):
            continue
        parsed = item["parsed"] if isinstance(item.get("parsed"), dict) else item
        qno = item.get("question_no") or parsed.get("question_no") or str(i + 1)
        out[str(qno)] = parsed
        align = item.get("align_key")
        if align:
            out[str(align)] = parsed
    return out


def lookup_parsed(by: dict[str, dict[str, Any]], qno: str) -> dict[str, Any] | None:
    if qno in by:
        return by[qno]
    if ":" in qno:
        return by.get(qno.split(":", 1)[1])
    return None


def chunk_for(paper_dir: Path, qno: str) -> tuple[str, str, str]:
    chunks_path = paper_dir / "chunks.jsonl"
    if chunks_path.is_file():
        for line in chunks_path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            try:
                row = json.loads(line)
            except json.JSONDecodeError:
                continue
            if not isinstance(row, dict):
                continue
            if str(row.get("question_no") or "") == str(qno):
                md = str(row.get("source_md") or "")
                src = str(row.get("slice_source") or "eval_reparse")
                via = str(row.get("split_via") or "")
                return md, src, via
    return "", "eval_reparse", ""


def gold_parsed(paper_dir: Path, qno: str) -> tuple[str, dict[str, Any] | None]:
    gold_path = paper_dir / "gold.json"
    if not gold_path.is_file():
        return "missing", None
    try:
        by = parsed_by_no(load_json(gold_path))
    except (OSError, json.JSONDecodeError, ValueError):
        return "missing", None
    if qno in by:
        return "gold", by[qno]
    got = lookup_parsed(by, qno)
    if got is not None:
        return "gold", got
    return "missing", None


def needs_l1(q: dict[str, Any]) -> bool:
    buckets = q.get("buckets") or {}
    if not isinstance(buckets, dict):
        return False
    for rec in buckets.values():
        if not isinstance(rec, dict):
            continue
        for side in rec.values():
            if isinstance(side, dict) and side.get("status") in ("fail", "na"):
                if side.get("status") == "fail":
                    return True
                reasons = side.get("reasons") or []
                if any("答案" in str(r) or "skip" in str(r).lower() for r in reasons):
                    return True
    return any(
        isinstance(rec, dict)
        and isinstance(rec.get("full"), dict)
        and rec["full"].get("status") == "fail"
        for rec in buckets.values()
        if isinstance(rec, dict)
    )


def sample_for_question(
    paper_dir: Path,
    report: dict[str, Any],
    q: dict[str, Any],
) -> dict[str, Any] | None:
    qno = str(q.get("question_no") or "")
    if not qno:
        return None
    buckets = q.get("buckets") if isinstance(q.get("buckets"), dict) else {}
    errors: list[dict[str, Any]] = []
    shunt_counts = report.get("shunt") if isinstance(report.get("shunt"), dict) else {}
    q_shunt = "ok"
    for kind in ("slice_fixture", "export_missed_prompt", "both_fail", "unaligned"):
        items = shunt_counts.get(kind) or []
        if any(isinstance(it, dict) and str(it.get("question_no")) == qno for it in items):
            q_shunt = kind
            break
    for bucket, sides in buckets.items():
        if not isinstance(sides, dict):
            continue
        full = sides.get("full") if isinstance(sides.get("full"), dict) else {}
        export = sides.get("export") if isinstance(sides.get("export"), dict) else {}
        fs, es = full.get("status"), export.get("status")
        if fs != "fail" and es != "fail":
            continue
        reasons = list(full.get("reasons") or []) + list(export.get("reasons") or [])
        evidence = "；".join(str(r) for r in reasons if r) or (q.get("paper_excerpt") or "")
        if fs == "fail" and es == "pass":
            shunt = "slice_fixture"
        elif fs == "pass" and es == "fail":
            shunt = "export_missed_prompt"
        elif fs == "fail" and es == "fail":
            shunt = "both_fail"
        elif fs == "fail":
            shunt = "slice_fixture"
        else:
            shunt = "export_missed_prompt"
        errors.append(
            {
                "tag": BUCKET_TAGS.get(bucket, "schema_error"),
                "rule_bucket": bucket,
                "stage": STAGE_OF.get(bucket, "llm"),
                "shunt": shunt,
                "evidence": evidence[:500],
            }
        )
    if not errors:
        return None
    meta = report.get("meta") if isinstance(report.get("meta"), dict) else load_meta(paper_dir)
    ocr = meta.get("ocr") if isinstance(meta.get("ocr"), dict) else {}
    prompt = meta.get("prompt") if isinstance(meta.get("prompt"), dict) else {}
    full_meta = meta.get("full") if isinstance(meta.get("full"), dict) else {}
    chunk_md, slice_source, split_via = chunk_for(paper_dir, qno)
    gold_src, gold = gold_parsed(paper_dir, qno)
    after: dict[str, Any] = {"source": gold_src, "parsed": gold}
    full_path = paper_dir / "full.json"
    export_path = paper_dir / "export.json"
    before_parsed = None
    export_parsed = None
    if full_path.is_file():
        try:
            before_parsed = lookup_parsed(parsed_by_no(load_json(full_path)), qno)
        except (OSError, json.JSONDecodeError, ValueError):
            before_parsed = None
    if export_path.is_file():
        try:
            export_parsed = lookup_parsed(parsed_by_no(load_json(export_path)), qno)
        except (OSError, json.JSONDecodeError, ValueError):
            export_parsed = None
    patch = prompt.get("stage2_patch") if isinstance(prompt.get("stage2_patch"), dict) else {}
    rules = prompt.get("rules_prompts") if isinstance(prompt.get("rules_prompts"), dict) else prompt
    sample = {
        "schema_version": SCHEMA,
        "paper_id": paper_dir.name,
        "question_no": qno,
        "document_id": meta.get("document_id"),
        "task_id": full_meta.get("task_id"),
        "ocr": {
            "engine": ocr.get("engine"),
            "markdown_sha256": ocr.get("markdown_sha256"),
            "layout_source": (full_meta.get("progress_summary") or {}).get("split_via")
            if isinstance(full_meta.get("progress_summary"), dict)
            else None,
        },
        "prompts": {
            "stage2_patch_sha256": patch.get("sha256"),
            "rules_prompts_sha256": rules.get("sha256"),
            "judge_doc": JUDGE_DOC,
        },
        "slice": {
            "split_via": split_via or None,
            "slice_source": slice_source,
            "chunk_md": chunk_md,
        },
        "before": {"path": "llm_merge", "parsed": before_parsed},
        "export": {"parsed": export_parsed} if export_parsed is not None else None,
        "after": after,
        "rule_score": {
            b: {
                "full": (sides.get("full") or {}).get("status"),
                "export": (sides.get("export") or {}).get("status"),
            }
            for b, sides in buckets.items()
            if isinstance(sides, dict)
        },
        "errors": errors,
        "shunt": q_shunt,
        "needs_fidelity_judge": list(FIDELITY_TAGS),
        "notes": "L1 仅根据 L0 桶映射标签；formula/semantic/hallucination 必须走保真裁判，正则不得假装能判。",
        "attributed_at": datetime.now(timezone.utc).isoformat(),
    }
    return sample


def attribute_paper(paper_dir: Path, out_dir: Path) -> int:
    report_path = paper_dir / "report.json"
    if not report_path.is_file():
        print(f"  跳过 {paper_dir.name}：没有 report.json（先跑 bench_eval_quality.py）")
        return 0
    try:
        report = load_json(report_path)
    except (OSError, json.JSONDecodeError) as e:
        print(f"  跳过 {paper_dir.name}：report.json {e}")
        return 0
    if report.get("import_failure"):
        print(f"  跳过 {paper_dir.name}：{report['import_failure']}")
        return 0
    questions = report.get("questions") or []
    samples: list[dict[str, Any]] = []
    for q in questions:
        if not isinstance(q, dict) or not needs_l1(q):
            continue
        sample = sample_for_question(paper_dir, report, q)
        if sample:
            samples.append(sample)
    out_dir.mkdir(parents=True, exist_ok=True)
    dest = out_dir / f"{paper_dir.name}.jsonl"
    dest.write_text(
        "".join(json.dumps(s, ensure_ascii=False) + "\n" for s in samples),
        encoding="utf-8",
    )
    print(f"  {paper_dir.name}：{len(samples)} 条 → {dest}")
    return len(samples)


def main() -> int:
    _stdio()
    parser = argparse.ArgumentParser(description="L0 fail → error_sample.v1 jsonl（不写回仓库代码）")
    parser.add_argument("--dir", default=str(DEFAULT_EVAL), help="试卷目录或评测根目录")
    parser.add_argument(
        "--out",
        default="",
        help="jsonl 输出目录（默认 <dir>/errors）",
    )
    args = parser.parse_args()
    root = Path(args.dir)
    if not root.is_absolute():
        root = (REPO_ROOT / root).resolve()
    dirs = list_paper_dirs(root)
    if not dirs:
        raise SystemExit(f"{root} 下没有含 paper.md 的试卷目录")
    out_dir = Path(args.out) if args.out else root / "errors"
    if not out_dir.is_absolute():
        out_dir = (REPO_ROOT / out_dir).resolve()
    total = 0
    for d in dirs:
        print(f"归因 {d.name} …")
        total += attribute_paper(d, out_dir)
    print(f"合计 {total} 条（裁判沟通词 {JUDGE_DOC}；未调用 LLM）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
