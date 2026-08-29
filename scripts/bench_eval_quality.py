#!/usr/bin/env python3
"""对照 paper.md / full.json / export.json 打规则分。

用法（仓库根目录）::

    python scripts/bench_eval_quality.py

    python scripts/bench_eval_quality.py --dir bench/eval/某试卷名

规则语义见 docs/全自动解析质量评估_评测规则.md。不调用 LLM，不改 slice / 提示词。
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import defaultdict
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_EVAL = REPO_ROOT / "bench" / "eval"
RULES_DOC = "docs/全自动解析质量评估_评测规则.md"

FW_DIGITS = str.maketrans("０１２３４５６７８９", "0123456789")
FW_BRACKETS = str.maketrans("（）", "()")

QUESTION_START = re.compile(
    r"^\s*(?:\*\*|__|#+\s*)?(?:第\s*)?([1-9]\d{0,2})\s*"
    r"(?:题|[.．、]\s|[.．、][\u4e00-\u9fff]|[.．、]$)"
)
SECTION_PREFIX = re.compile(
    r"^[（(]?[一二三四五六七八九十]+[)）]?[、.．]\s*"
)
INSTRUCTION_HINTS = (
    "答卷前",
    "考生务必",
    "准考证",
    "答题卡",
    "用铅笔",
    "用橡皮",
    "本试卷",
    "写在本试卷",
    "考试结束",
    "一并交回",
    "密封线",
    "填涂",
    "选出每小题",
    "回答选择题时",
    "注意事项",
)
EDITORIAL_TAG = re.compile(r"【\s*(分析|详解|解析|答案|解答|点睛)[^】]*】")
EDITORIAL_TITLES = {"分析", "点睛", "详解"}
STRATEGY_HINTS = (
    "即可得",
    "即可求",
    "即可解",
    "即可判断",
    "即可选",
    "即可求解",
    "可求",
    "解出即可",
    "根据图象即可",
    "由递推即可",
)
OPTION_IN_STEM = re.compile(
    r"(?m)(?:^|[\s$）)])A\s*[\.．、\)]\s+\S"
)
SUBQ_MARK = re.compile(r"[（(]\s*([1-9]|[一二三四五六七八九十])\s*[)）]")
SUBQ_SPLIT = re.compile(r"[（(]\s*(?:[1-9]|[一二三四五六七八九十]+)\s*[)）]")
CHOICE_LINE = re.compile(
    r"(?m)^\s*A\s*[\.．、\)]|[\s$）)]A\s*[\.．、\)]\s"
)
ANS_MATHRM = re.compile(
    r"(?:【\s*答案\s*】|\[\s*答案\s*\]|故选|答案)[:：]?\s*\$?\\mathrm\s*\{([A-Da-d]+)\}"
)
ANS_BRACKET = re.compile(
    r"【\s*答案\s*】\s*[:：]?\s*\$?\s*([A-Da-d,，、\s]{1,12})"
)
ANS_SQUARE = re.compile(
    r"\[\s*答案\s*\]\s*[:：]?\s*\$?\s*([A-Da-d,，、\s]{1,12})"
)
GU_XUAN = re.compile(r"故选[:：]?\s*\$?\\mathrm\s*\{([A-Da-d]+)\}|故选[:：]?\s*([A-Da-d,，、\s]{1,4})")
ANS_BLOCK = re.compile(
    r"【\s*答案\s*】\s*[:：]?\s*(.+?)(?=【\s*(?:解析|详解|分析)|$)",
    re.S,
)
BAD_DELIM = re.compile(r"\\\(|\\\)|\\\[|\\\]")
IMG_HTML = re.compile(r"<img\b", re.I)
VALID_TYPES = {"choice", "fill", "solution", "multiple"}
BUCKETS = (
    "cut",
    "choice_schema",
    "answer",
    "parts",
    "editorial",
    "schema",
)


def _stdio() -> None:
    for stream in (sys.stdout, sys.stderr):
        reconf = getattr(stream, "reconfigure", None)
        if reconf is not None:
            try:
                reconf(encoding="utf-8")
            except Exception:
                pass


def fw_half(s: str) -> str:
    return s.translate(FW_DIGITS).translate(FW_BRACKETS)


def normalize_question_no(raw: Any) -> str | None:
    if raw is None:
        return None
    s = fw_half(str(raw)).strip()
    if not s:
        return None
    s = s.replace(" ", "")
    s = re.sub(r"^第", "", s)
    s = re.sub(r"题$", "", s)
    s = SECTION_PREFIX.sub("", s)
    s = s.rstrip(".．、.")
    m = re.match(r"^(\d+)(?:\((\d+)\))?$", s)
    if m:
        return f"{int(m.group(1))}" + (f"({int(m.group(2))})" if m.group(2) else "")
    m = re.search(r"(\d+)", s)
    if m:
        return str(int(m.group(1)))
    return s or None


def parse_no_key(raw: str | None) -> tuple[int, int]:
    n = normalize_question_no(raw) or ""
    m = re.match(r"^(\d+)(?:\((\d+)\))?$", n)
    if not m:
        return (10**9, 0)
    return (int(m.group(1)), int(m.group(2) or 0))


def parse_align_key(raw: str | None) -> tuple[int, int, int]:
    s = str(raw or "")
    if ":" in s:
        left, right = s.split(":", 1)
        try:
            sec = int(left)
        except ValueError:
            sec = 10**9
        a, b = parse_no_key(right)
        return (sec, a, b)
    a, b = parse_no_key(s)
    return (0, a, b)


def is_instruction_line(line: str) -> bool:
    return any(h in line for h in INSTRUCTION_HINTS)


def paper_question_starts(md: str) -> list[tuple[int, int, str, int]]:
    """(char_offset, section, major_no, line_index)。编号从 1 再起或遇到「一、」大题头则新开一段。"""
    out: list[tuple[int, int, str, int]] = []
    offset = 0
    prev = 0
    section = 0
    just_header = False
    for li, line in enumerate(md.splitlines(keepends=True)):
        body = line.rstrip("\r\n")
        trimmed = body.strip()
        if trimmed and not is_instruction_line(trimmed):
            header = SECTION_PREFIX.match(trimmed)
            qmatch = QUESTION_START.match(trimmed)
            if header and not qmatch:
                section += 1
                just_header = True
                prev = 0
                offset += len(line)
                continue
            if qmatch:
                n = int(qmatch.group(1))
                if prev >= 10 and n <= 9 and n != 1 and n < prev:
                    offset += len(line)
                    continue
                if n == 1 and prev >= 1 and not just_header:
                    section += 1
                just_header = False
                out.append((offset, section, str(n), li))
                prev = n
        offset += len(line)
    return out


def slice_paper(md: str) -> dict[str, str]:
    """题号键为 `{段}:{题号}`，填空从 1 再起不会覆盖选择题 1。"""
    starts = paper_question_starts(md)
    slices: dict[str, str] = {}
    for i, (off, section, no, _) in enumerate(starts):
        end = starts[i + 1][0] if i + 1 < len(starts) else len(md)
        key = align_key(section, normalize_question_no(no) or no)
        slices[key] = md[off:end]
    return slices


def align_key(section: int, no: str | None) -> str:
    n = no or "?"
    return f"{section}:{n}"


def load_json(path: Path) -> Any:
    return json.loads(path.read_text(encoding="utf-8-sig"))


def unwrap_questions(payload: Any) -> list[dict[str, Any]]:
    if isinstance(payload, list):
        items = payload
    elif isinstance(payload, dict):
        items = payload.get("questions")
        if not isinstance(items, list):
            raise ValueError("JSON 须含 questions 数组")
    else:
        raise ValueError("JSON 须为对象或数组")
    out: list[dict[str, Any]] = []
    for i, item in enumerate(items):
        if not isinstance(item, dict):
            continue
        if isinstance(item.get("parsed"), dict):
            parsed = item["parsed"]
            qno = item.get("question_no") or parsed.get("question_no")
            order = item.get("display_order")
            if order is None:
                order = parsed.get("display_order")
        else:
            parsed = item
            qno = item.get("question_no")
            order = item.get("display_order")
        if order is None:
            order = i + 1
        out.append(
            {
                "question_no": qno,
                "display_order": order,
                "index": item.get("index"),
                "parsed": parsed,
            "norm": normalize_question_no(qno),
            "align_key": None,
            "section": 0,
        }
        )
    assign_align_keys(out)
    return out


def assign_align_keys(items: list[dict[str, Any]]) -> None:
    """按 display_order 给 JSON 题号分段，与 slice_paper 的 `{段}:{题号}` 对齐。"""
    ordered = sorted(
        items,
        key=lambda x: (
            as_order(x.get("display_order")) if as_order(x.get("display_order")) is not None else 10**9,
            parse_no_key(x.get("norm")),
        ),
    )
    section = 0
    prev = 0
    started = False
    for it in ordered:
        n, _ = parse_no_key(it.get("norm"))
        if started and n == 1 and prev >= 1:
            section += 1
        it["section"] = section
        it["align_key"] = align_key(section, it.get("norm")) if it.get("norm") else None
        if n < 10**9:
            prev = n
            started = True


def iter_analysis_items(parsed: dict[str, Any]):
    for a in parsed.get("analysis") or []:
        if isinstance(a, dict):
            yield a
        elif a is not None:
            yield {"title": "", "content": str(a)}
    stack = list(parsed.get("parts") or [])
    while stack:
        p = stack.pop()
        if not isinstance(p, dict):
            continue
        for a in p.get("analyses") or []:
            if isinstance(a, dict):
                yield a
        stack.extend(p.get("children") or [])


def analysis_blob(parsed: dict[str, Any]) -> str:
    parts: list[str] = []
    for a in iter_analysis_items(parsed):
        parts.append(str(a.get("title") or ""))
        parts.append(str(a.get("content") or ""))
    return "\n".join(parts)


def analysis_titles(parsed: dict[str, Any]) -> list[str]:
    titles: list[str] = []
    for a in iter_analysis_items(parsed):
        t = str(a.get("title") or "").strip()
        if t:
            titles.append(t)
    return titles


def iter_leaves(parsed: dict[str, Any]):
    stack = list(parsed.get("parts") or [])
    while stack:
        p = stack.pop()
        if not isinstance(p, dict):
            continue
        children = p.get("children") or []
        if children:
            stack.extend(children)
            continue
        yield p


def leaf_answers(parsed: dict[str, Any]) -> list[str]:
    found: list[str] = []
    for p in iter_leaves(parsed):
        ans = p.get("answer")
        if isinstance(ans, str) and ans.strip():
            found.append(ans.strip())
    return found


def leaf_has_payload(parsed: dict[str, Any]) -> bool:
    for p in iter_leaves(parsed):
        ans = p.get("answer")
        if isinstance(ans, str) and ans.strip():
            return True
        for a in p.get("analyses") or []:
            if isinstance(a, dict) and str(a.get("content") or "").strip():
                return True
    return False


def choice_letters(parsed: dict[str, Any]) -> list[str]:
    ca = parsed.get("correct_answer")
    if not isinstance(ca, dict):
        return []
    val = ca.get("value") if isinstance(ca.get("value"), dict) else ca
    opts = val.get("options") if isinstance(val, dict) else None
    if not isinstance(opts, list):
        return []
    letters: list[str] = []
    for o in opts:
        s = fw_half(str(o)).strip().upper()
        for ch in s:
            if ch in "ABCD" and ch not in letters:
                letters.append(ch)
    return sorted(letters)


def option_labels(parsed: dict[str, Any]) -> list[str]:
    opts = parsed.get("options")
    if not isinstance(opts, list):
        return []
    labels: list[str] = []
    for o in opts:
        if isinstance(o, dict):
            lab = str(o.get("label") or "").strip().upper()
        else:
            lab = str(o).strip().upper()[:1]
        if lab:
            labels.append(lab)
    return labels


def qtype(parsed: dict[str, Any]) -> str:
    t = str(parsed.get("question_type") or "").strip().lower()
    if t == "multiple":
        return "choice"
    return t


def letters_from_text(blob: str) -> list[str]:
    blob = fw_half(blob)
    m = ANS_MATHRM.search(blob)
    if m:
        return sorted({c.upper() for c in m.group(1) if c.upper() in "ABCD"})
    for rx in (ANS_BRACKET, ANS_SQUARE):
        m = rx.search(blob)
        if m:
            return sorted({c.upper() for c in m.group(1) if c.upper() in "ABCD"})
    m = GU_XUAN.search(blob)
    if m:
        raw = m.group(1) or m.group(2) or ""
        return sorted({c.upper() for c in raw if c.upper() in "ABCD"})
    return []


def paper_answer_text(slice_md: str) -> str:
    m = ANS_BLOCK.search(slice_md)
    if not m:
        return ""
    return m.group(1).strip()


def norm_latex(s: str) -> str:
    t = fw_half(s)
    t = t.replace("\\dfrac", "\\frac").replace("\\mathrm", "")
    t = re.sub(r"[{}\s$]", "", t)
    return t.lower()


def strategy_heads(text: str) -> list[str]:
    t = text.strip()
    if not t:
        return []
    heads = [t]
    if "\n" in t:
        heads.append(t.split("\n", 1)[0].strip())
    for sep in ("\n\n", "；"):
        if sep in t:
            heads.append(t.split(sep, 1)[0].strip())
    seen: list[str] = []
    for h in heads:
        if h and h not in seen:
            seen.append(h)
    return seen


def looks_strategy(text: str) -> bool:
    t = text.strip()
    if not t or len(t) > 200:
        return False
    if "故选" in t or "故填" in t:
        return False
    if "因为" in t and "所以" in t:
        return False
    if any(h in t for h in STRATEGY_HINTS):
        return True
    stripped = t.rstrip("。．. ")
    if stripped.startswith("根据") and stripped.endswith("即可"):
        return True
    return False


def paper_is_choice(slice_md: str) -> bool:
    """只认选项表：行首连续 A./A、 至少两行，或同行 A. … B. … C.。"""
    head = slice_md.split("【")[0]
    labels: list[str] = []
    for line in head.splitlines():
        m = re.match(r"^\s*([A-Da-d])\s*[\.．、\)]\s+\S", line)
        if m:
            labels.append(m.group(1).upper())
            continue
        if labels and line.strip():
            break
    if len(labels) >= 2 and labels[0] == "A":
        return True
    return bool(
        re.search(
            r"(?m)^\s*A\s*[\.．、\)]\s+\S.{0,120}B\s*[\.．、\)]\s+\S.{0,120}C\s*[\.．、\)]",
            head,
        )
    )


def paper_subq_count(slice_md: str) -> int:
    head = re.split(r"【\s*(答案|解析|详解|分析)", slice_md, maxsplit=1)[0]
    marks = SUBQ_MARK.findall(head)
    return len(marks)


def score_schema(parsed: dict[str, Any]) -> tuple[str, list[str]]:
    reasons: list[str] = []
    t = str(parsed.get("question_type") or "").strip().lower()
    if t not in VALID_TYPES:
        reasons.append(f"question_type={t!r}")
    ca = parsed.get("correct_answer")
    if ca is None:
        reasons.append("correct_answer 为 null")
    elif not isinstance(ca, dict) or "kind" not in ca or "value" not in ca:
        reasons.append("correct_answer 缺少 kind/value")
    blob = json.dumps(parsed, ensure_ascii=False)
    if BAD_DELIM.search(blob):
        reasons.append("含 \\( \\) \\[ \\]")
    if IMG_HTML.search(blob):
        reasons.append("含 <img>")
    return ("fail" if reasons else "pass", reasons)


def score_choice_schema(parsed: dict[str, Any], slice_md: str) -> tuple[str, list[str]]:
    if not (paper_is_choice(slice_md) or qtype(parsed) == "choice"):
        return "na", []
    reasons: list[str] = []
    t = qtype(parsed)
    if t != "choice":
        reasons.append(f"题型应为 choice，实际 {parsed.get('question_type')!r}")
    labels = option_labels(parsed)
    if len(labels) < 2:
        reasons.append(f"options 项数不足（{len(labels)}）")
    stem = str(parsed.get("stem") or "")
    if OPTION_IN_STEM.search(stem):
        reasons.append("stem 残留选项 A.")
    if "多选" in slice_md[:200] or len(letters_from_text(slice_md)) >= 2:
        sub = str(parsed.get("sub_type") or "").lower()
        raw_t = str(parsed.get("question_type") or "").lower()
        if sub != "multi" and raw_t != "multiple":
            reasons.append("多选未标 sub_type=multi")
    return ("fail" if reasons else "pass", reasons)


def split_answer_segments(text: str) -> list[str]:
    t = text.strip()
    if not t:
        return []
    parts = [p.strip() for p in SUBQ_SPLIT.split(t)]
    parts = [p for p in parts if p]
    return parts or [t]


def answers_exact_match(gold_text: str, cands: list[str]) -> bool:
    gold_segs = [norm_latex(s) for s in split_answer_segments(gold_text)]
    gold_segs = [s for s in gold_segs if s]
    cand_segs = [norm_latex(c) for c in cands if c]
    cand_segs = [s for s in cand_segs if s]
    if not gold_segs or not cand_segs:
        return False
    if len(gold_segs) == 1:
        return gold_segs[0] in cand_segs
    return gold_segs == cand_segs


def score_answer(parsed: dict[str, Any], slice_md: str) -> tuple[str, list[str]]:
    printed = letters_from_text(slice_md)
    if printed:
        got = choice_letters(parsed)
        if got == printed:
            return "pass", []
        return "fail", [f"印刷答案 {''.join(printed)}，JSON 为 {''.join(got) or '空'}"]
    gold = paper_answer_text(slice_md)
    if not gold.strip():
        return "na", []
    gold_n = norm_latex(gold)
    if not gold_n:
        return "na", []
    cands = leaf_answers(parsed)
    ca = parsed.get("correct_answer")
    if isinstance(ca, dict):
        val = ca.get("value")
        if isinstance(val, dict):
            blanks = val.get("blanks")
            if isinstance(blanks, list):
                for b in blanks:
                    if isinstance(b, dict) and b.get("answer"):
                        cands.append(str(b["answer"]))
            subs = val.get("subs")
            if isinstance(subs, list):
                for s in subs:
                    if isinstance(s, dict) and s.get("answer"):
                        cands.append(str(s["answer"]))
                    elif isinstance(s, str):
                        cands.append(s)
    if answers_exact_match(gold, cands):
        return "pass", []
    if cands:
        return "fail", ["叶子答案与【答案】分段全等对不上"]
    if qtype(parsed) == "choice":
        return "na", []
    return "fail", ["无叶子答案可与卷面【答案】对照"]


def score_parts(parsed: dict[str, Any], slice_md: str) -> tuple[str, list[str]]:
    if paper_is_choice(slice_md):
        return "na", []
    n_sub = paper_subq_count(slice_md)
    if n_sub < 2:
        return "na", []
    raw_t = str(parsed.get("question_type") or "").strip().lower()
    if raw_t == "fill":
        return "na", []
    reasons: list[str] = []
    if qtype(parsed) != "solution":
        reasons.append(f"有（1）（2）但题型为 {parsed.get('question_type')!r}")
    parts = parsed.get("parts") if isinstance(parsed.get("parts"), list) else []
    if not parts:
        reasons.append("parts 为空")
    elif not leaf_has_payload(parsed):
        reasons.append("叶子缺少 answer 或 analyses")
    stem = str(parsed.get("stem") or "")
    if SUBQ_MARK.search(stem) and len(stem) > 40:
        reasons.append("小问正文仍在整题 stem")
    analysis = parsed.get("analysis") if isinstance(parsed.get("analysis"), list) else []
    if analysis:
        reasons.append("解答题整题 analysis 应为空")
    return ("fail" if reasons else "pass", reasons)


def score_editorial(parsed: dict[str, Any]) -> tuple[str, list[str]]:
    reasons: list[str] = []
    blob = analysis_blob(parsed)
    tags = sorted({m.group(1) for m in EDITORIAL_TAG.finditer(blob)})
    if tags:
        reasons.append("解析残留【" + "】【".join(tags) + "】")
    extra = [
        t
        for t in analysis_titles(parsed)
        if t in EDITORIAL_TITLES or "详解" in t
    ]
    extra = list(dict.fromkeys(extra))
    if extra:
        reasons.append("解析 title 为教辅标签：" + "、".join(extra[:6]))
    for a in iter_analysis_items(parsed):
        if any(looks_strategy(h) for h in strategy_heads(str(a.get("content") or ""))):
            reasons.append("短思路摘要（根据…即可/可求）")
            break
    return ("fail" if reasons else "pass", reasons)


def score_side(parsed: dict[str, Any], slice_md: str) -> dict[str, Any]:
    schema, sr = score_schema(parsed)
    choice, cr = score_choice_schema(parsed, slice_md)
    answer, ar = score_answer(parsed, slice_md)
    parts, pr = score_parts(parsed, slice_md)
    editorial, er = score_editorial(parsed)
    return {
        "schema": {"status": schema, "reasons": sr},
        "choice_schema": {"status": choice, "reasons": cr},
        "answer": {"status": answer, "reasons": ar},
        "parts": {"status": parts, "reasons": pr},
        "editorial": {"status": editorial, "reasons": er},
    }


def index_by_norm(items: list[dict[str, Any]]) -> tuple[dict[str, dict[str, Any]], list[dict[str, Any]], list[str]]:
    by: dict[str, list[dict[str, Any]]] = defaultdict(list)
    no_key: list[dict[str, Any]] = []
    dups: list[str] = []
    for it in items:
        k = it.get("align_key") or it.get("norm")
        if k:
            by[k].append(it)
        else:
            no_key.append(it)
    unique: dict[str, dict[str, Any]] = {}
    for k, grp in by.items():
        unique[k] = grp[0]
        if len(grp) > 1:
            dups.append(k)
    return unique, no_key, dups


def align(
    full_items: list[dict[str, Any]],
    export_items: list[dict[str, Any]],
) -> dict[str, Any]:
    f_map, f_rest, f_dup = index_by_norm(full_items)
    e_map, e_rest, e_dup = index_by_norm(export_items)
    pairs: list[dict[str, Any]] = []
    used_e: set[str] = set()
    used_f: set[str] = set()
    for k in sorted(set(f_map) | set(e_map), key=lambda x: parse_align_key(x)):
        if k in f_map and k in e_map:
            pairs.append(
                {
                    "question_no": k,
                    "aligned_by": "question_no",
                    "full": f_map[k],
                    "export": e_map[k],
                }
            )
            used_f.add(k)
            used_e.add(k)
    f_left = [f_map[k] for k in f_map if k not in used_f] + f_rest
    e_left = [e_map[k] for k in e_map if k not in used_e] + e_rest
    f_by_ord: dict[int, dict[str, Any]] = {}
    e_by_ord: dict[int, dict[str, Any]] = {}
    for x in f_left:
        od = as_order(x.get("display_order"))
        if od is not None:
            f_by_ord[od] = x
    for x in e_left:
        od = as_order(x.get("display_order"))
        if od is not None:
            e_by_ord[od] = x
    for od in sorted(set(f_by_ord) & set(e_by_ord)):
        fv, ev = f_by_ord[od], e_by_ord[od]
        pairs.append(
            {
                "question_no": fv.get("norm") or ev.get("norm") or str(od),
                "aligned_by": "order",
                "full": fv,
                "export": ev,
            }
        )
        f_left = [x for x in f_left if x is not fv]
        e_left = [x for x in e_left if x is not ev]
    return {
        "pairs": pairs,
        "full_only": f_left,
        "export_only": e_left,
        "full_dup": f_dup,
        "export_dup": e_dup,
    }


def verdict(full_st: str, export_st: str) -> str:
    if full_st == "na" and export_st == "na":
        return "na"
    if full_st == "fail" and export_st == "pass":
        return "slice_fixture"
    if full_st == "pass" and export_st == "fail":
        return "export_missed_prompt"
    if full_st == "fail" and export_st == "fail":
        return "both_fail"
    if full_st == "fail":
        return "slice_fixture"
    if export_st == "fail":
        return "export_missed_prompt"
    return "ok"


def excerpt(text: str, n: int = 120) -> str:
    t = re.sub(r"\s+", " ", text).strip()
    return t if len(t) <= n else t[: n - 1] + "…"


def as_order(raw: Any) -> int | None:
    try:
        return int(raw)
    except (TypeError, ValueError):
        return None


def relpath(path: Path) -> str:
    try:
        return str(path.resolve().relative_to(REPO_ROOT)).replace("\\", "/")
    except ValueError:
        return str(path).replace("\\", "/")


def cut_status(
    no: str,
    *,
    paper_nos: set[str],
    side_nos: set[str],
    dups: set[str],
) -> tuple[str, list[str]]:
    reasons: list[str] = []
    if no in dups:
        reasons.append("同一题号出现多次")
    if no in paper_nos and no not in side_nos:
        reasons.append("卷面有此题，该侧没有")
    if no not in paper_nos and no in side_nos:
        reasons.append("卷面未识别此题号，该侧多出")
    return ("fail" if reasons else "pass"), reasons


def eval_paper(paper_dir: Path) -> dict[str, Any]:
    paper_md = paper_dir / "paper.md"
    full_path = paper_dir / "full.json"
    export_path = paper_dir / "export.json"
    meta_path = paper_dir / "meta.json"
    result: dict[str, Any] = {
        "schema_version": "1",
        "rules": RULES_DOC,
        "dir": relpath(paper_dir),
        "name": paper_dir.name,
        "evaluated_at": datetime.now(timezone.utc).isoformat(),
        "import_failure": None,
    }
    if meta_path.is_file():
        try:
            result["meta"] = load_json(meta_path)
        except json.JSONDecodeError:
            result["meta"] = None
    else:
        result["meta"] = None

    if not paper_md.is_file() or not full_path.is_file():
        result["import_failure"] = "缺少 paper.md 或 full.json"
        return result
    md = paper_md.read_text(encoding="utf-8")
    try:
        full_items = unwrap_questions(load_json(full_path))
    except (OSError, json.JSONDecodeError, ValueError) as e:
        result["import_failure"] = f"full.json：{e}"
        return result
    export_absent = not export_path.is_file()
    export_items: list[dict[str, Any]] = []
    if export_absent:
        result["export_absent"] = True
    else:
        result["export_absent"] = False
        try:
            export_items = unwrap_questions(load_json(export_path))
        except (OSError, json.JSONDecodeError, ValueError) as e:
            result["import_failure"] = f"export.json 无法解析：{e}"
            return result

    slices = slice_paper(md)
    paper_nos = set(slices)

    def item_key(it: dict[str, Any]) -> str | None:
        return it.get("align_key") or it.get("norm")

    full_nos = {item_key(x) for x in full_items if item_key(x)}
    export_nos = {item_key(x) for x in export_items if item_key(x)} if not export_absent else set()
    al = align(full_items, export_items)

    cut_full_fail = sorted(paper_nos - full_nos, key=parse_align_key) + [
        f"dup:{x}" for x in al["full_dup"]
    ]
    cut_export_fail = (
        []
        if export_absent
        else sorted(paper_nos - export_nos, key=parse_align_key)
        + [f"dup:{x}" for x in al["export_dup"]]
    )
    extra_full = sorted(full_nos - paper_nos, key=parse_align_key)
    extra_export = [] if export_absent else sorted(export_nos - paper_nos, key=parse_align_key)
    full_dups = set(al["full_dup"])
    export_dups = set() if export_absent else set(al["export_dup"])

    if export_absent:
        f_map, f_rest, _ = index_by_norm(full_items)
        al["pairs"] = [
            {
                "question_no": k,
                "aligned_by": "question_no",
                "full": item,
                "export": None,
            }
            for k, item in sorted(f_map.items(), key=lambda kv: parse_align_key(kv[0]))
        ]
        al["full_only"] = f_rest
        al["export_only"] = []
        al["export_dup"] = []

    questions: list[dict[str, Any]] = []
    bucket_counts: dict[str, dict[str, int]] = {
        b: {"full_fail": 0, "export_fail": 0, "both_fail": 0, "n": 0} for b in BUCKETS
    }
    shunt = {
        "slice_fixture": [],
        "export_missed_prompt": [],
        "both_fail": [],
        "unaligned": [],
    }
    seen_nos: set[str] = set()

    def bump(bucket: str, fs: str, es: str) -> None:
        if fs == "na" and es == "na":
            return
        bucket_counts[bucket]["n"] += 1
        if fs == "fail":
            bucket_counts[bucket]["full_fail"] += 1
        if es == "fail":
            bucket_counts[bucket]["export_fail"] += 1
        if fs == "fail" and es == "fail":
            bucket_counts[bucket]["both_fail"] += 1

    def note(
        qno: str,
        bucket: str,
        fs: str,
        es: str,
        *,
        fr: list[str] | None = None,
        er: list[str] | None = None,
    ) -> None:
        bump(bucket, fs, es)
        vv = verdict(fs, es)
        if export_absent and vv == "export_missed_prompt":
            return
        if vv in ("slice_fixture", "export_missed_prompt", "both_fail"):
            shunt[vv].append(
                {
                    "question_no": qno,
                    "bucket": bucket,
                    "full_reasons": fr or [],
                    "export_reasons": er or [],
                }
            )

    def sides_cut(no: str, aligned_by: str | None) -> tuple[dict[str, Any], dict[str, Any]]:
        fs, fr = cut_status(no, paper_nos=paper_nos, side_nos=full_nos, dups=full_dups)
        if export_absent:
            es, er = "na", ["无 export.json"]
        else:
            es, er = cut_status(no, paper_nos=paper_nos, side_nos=export_nos, dups=export_dups)
        if aligned_by == "order":
            fs = "fail"
            fr = ["仅按 display_order 对齐，题号不可靠"]
            if not export_absent:
                es, er = "fail", fr
        return {"status": fs, "reasons": fr}, {"status": es, "reasons": er}

    for pair in al["pairs"]:
        qno = pair["question_no"]
        seen_nos.add(qno)
        sl = slices.get(qno, "")
        if not sl:
            sl = slices.get(pair["full"].get("align_key") or "", "") or slices.get(
                pair["full"].get("norm") or "", ""
            )
        fp = pair["full"]["parsed"]
        export_item = pair.get("export")
        fscores = score_side(fp, sl)
        if export_item is None:
            na = {"status": "na", "reasons": ["无 export.json"]}
            escores = {b: na for b in ("choice_schema", "answer", "parts", "editorial", "schema")}
        else:
            escores = score_side(export_item["parsed"], sl)
        cut_f, cut_e = sides_cut(qno, pair["aligned_by"])
        row_buckets: dict[str, Any] = {"cut": {"full": cut_f, "export": cut_e}}
        note(qno, "cut", cut_f["status"], cut_e["status"], fr=cut_f["reasons"], er=cut_e["reasons"])
        if pair["aligned_by"] != "question_no":
            shunt["unaligned"].append({"question_no": qno, "reason": "aligned_by_order"})
        for b in ("choice_schema", "answer", "parts", "editorial", "schema"):
            if pair["aligned_by"] != "question_no":
                row_buckets[b] = {
                    "full": {"status": "na", "reasons": ["未按题号对齐，细项不适用"]},
                    "export": {"status": "na", "reasons": ["未按题号对齐，细项不适用"]},
                }
                continue
            fb, eb = fscores[b], escores[b]
            row_buckets[b] = {"full": fb, "export": eb}
            note(qno, b, fb["status"], eb["status"], fr=fb["reasons"], er=eb["reasons"])
        questions.append(
            {
                "question_no": qno,
                "aligned_by": pair["aligned_by"],
                "buckets": row_buckets,
                "stem_full": excerpt(str(fp.get("stem") or "")),
                "stem_export": excerpt(str((export_item or {}).get("parsed", {}).get("stem") or ""))
                if export_item
                else "",
                "paper_excerpt": excerpt(sl, 160),
            }
        )

    for item in al["full_only"]:
        qno = item.get("align_key") or item.get("norm") or "?"
        seen_nos.add(qno)
        cut_f, cut_e = sides_cut(qno, None)
        note(qno, "cut", cut_f["status"], cut_e["status"], fr=cut_f["reasons"], er=cut_e["reasons"])
        if qno not in paper_nos:
            shunt["unaligned"].append({"side": "full", "question_no": qno})
        questions.append(
            {
                "question_no": qno,
                "aligned_by": None,
                "only": "full",
                "buckets": {"cut": {"full": cut_f, "export": cut_e}},
            }
        )
    for item in al["export_only"]:
        qno = item.get("align_key") or item.get("norm") or "?"
        seen_nos.add(qno)
        cut_f, cut_e = sides_cut(qno, None)
        note(qno, "cut", cut_f["status"], cut_e["status"], fr=cut_f["reasons"], er=cut_e["reasons"])
        if qno not in paper_nos:
            shunt["unaligned"].append({"side": "export", "question_no": qno})
        questions.append(
            {
                "question_no": qno,
                "aligned_by": None,
                "only": "export",
                "buckets": {"cut": {"full": cut_f, "export": cut_e}},
            }
        )
    for qno in sorted(paper_nos - seen_nos, key=parse_align_key):
        cut_f, cut_e = sides_cut(qno, None)
        note(qno, "cut", cut_f["status"], cut_e["status"], fr=cut_f["reasons"], er=cut_e["reasons"])
        questions.append(
            {
                "question_no": qno,
                "aligned_by": None,
                "only": "paper",
                "buckets": {"cut": {"full": cut_f, "export": cut_e}},
            }
        )

    rates: dict[str, Any] = {}
    for b, c in bucket_counts.items():
        n = c["n"] or 1
        rates[b] = {
            **c,
            "full_fail_rate": round(c["full_fail"] / n, 4) if c["n"] else None,
            "export_fail_rate": round(c["export_fail"] / n, 4) if c["n"] else None,
            "both_fail_rate": round(c["both_fail"] / n, 4) if c["n"] else None,
        }

    questions.sort(key=lambda q: parse_align_key(str(q.get("question_no") or "")))

    result.update(
        {
            "paper_question_nos": sorted(paper_nos, key=parse_align_key),
            "full_count": len(full_items),
            "export_count": 0 if export_absent else len(export_items),
            "aligned": sum(1 for p in al["pairs"] if p["aligned_by"] == "question_no"),
            "aligned_by_order": sum(1 for p in al["pairs"] if p["aligned_by"] == "order"),
            "full_only_n": len(al["full_only"]),
            "export_only_n": len(al["export_only"]),
            "cut": {
                "paper_missing_in_full": cut_full_fail,
                "paper_missing_in_export": cut_export_fail,
                "full_not_in_paper": extra_full,
                "export_not_in_paper": extra_export,
            },
            "bucket_rates": rates,
            "shunt_counts": {k: len(v) for k, v in shunt.items()},
            "shunt": shunt,
            "questions": questions,
        }
    )
    return result


def write_report_md(data: dict[str, Any]) -> str:
    lines = [
        f"# 评测报告 · {data.get('name', '')}",
        "",
        f"规则：`{RULES_DOC}`",
        f"时间：{data.get('evaluated_at', '')}",
        "",
    ]
    if data.get("import_failure"):
        lines += [f"**导入失败：** {data['import_failure']}", ""]
        return "\n".join(lines)
    export_note = "（本卷无 export.json，只评全自动）" if data.get("export_absent") else ""
    lines += [
        f"- 卷面题号 {len(data.get('paper_question_nos') or [])}：`{'、'.join(data.get('paper_question_nos') or [])}`",
        f"- 全自动 {data.get('full_count')} 题，站外 {data.get('export_count')} 题{export_note}",
        f"- 按题号对齐 {data.get('aligned')}，按序号对齐 {data.get('aligned_by_order')}，仅全自动 {data.get('full_only_n')}，仅站外 {data.get('export_only_n')}",
        "",
        "## 各桶失败率",
        "",
        "| 桶 | 样本 | 全自动失败 | 站外失败 | 两边同失败 |",
        "|----|------|------------|----------|------------|",
    ]
    names = {
        "cut": "切题",
        "choice_schema": "选择题结构",
        "answer": "答案",
        "parts": "解答题树",
        "editorial": "教辅杂质",
        "schema": "JSON 契约",
    }
    for b in BUCKETS:
        c = (data.get("bucket_rates") or {}).get(b) or {}
        lines.append(
            f"| {names.get(b, b)} | {c.get('n', 0)} | "
            f"{c.get('full_fail', 0)}（{c.get('full_fail_rate') or '-'}） | "
            f"{c.get('export_fail', 0)}（{c.get('export_fail_rate') or '-'}） | "
            f"{c.get('both_fail', 0)} |"
        )
    sc = data.get("shunt_counts") or {}
    lines += [
        "",
        "## 分流（不自动改代码）",
        "",
        f"- 应进 slice 夹具（站外对、全自动错）：**{sc.get('slice_fixture', 0)}**",
        f"- 站外未遵守提示词（全自动对、站外错）：**{sc.get('export_missed_prompt', 0)}**",
        f"- 两边同错（先排除 OCR，再考虑改提示词）：**{sc.get('both_fail', 0)}**",
        f"- 未对齐：**{sc.get('unaligned', 0)}**",
        "",
        "## 题级明细（失败项）",
        "",
    ]
    any_fail = False
    for q in data.get("questions") or []:
        bits: list[str] = []
        buckets = q.get("buckets") or {}
        for b, sides in buckets.items():
            if not isinstance(sides, dict):
                continue
            for side, rec in sides.items():
                if isinstance(rec, dict) and rec.get("status") == "fail":
                    why = "；".join(rec.get("reasons") or [])
                    side_name = {"full": "全自动", "export": "站外"}.get(side, side)
                    bits.append(f"{b}/{side_name}: {why}")
        if not bits:
            continue
        any_fail = True
        only_names = {"full": "仅全自动", "export": "仅站外", "paper": "仅卷面"}
        how = q.get("aligned_by") or only_names.get(q.get("only"), q.get("only") or "-")
        lines.append(f"### 题 {q.get('question_no')}（对齐 {how}）")
        for bit in bits:
            lines.append(f"- {bit}")
        if q.get("paper_excerpt"):
            lines.append(f"- 卷面摘录：{q['paper_excerpt']}")
        lines.append("")
    if not any_fail:
        lines.append("（对齐题上各桶均未 fail）")
        lines.append("")
    cut = data.get("cut") or {}
    if any(cut.get(k) for k in cut):
        lines += ["## 切题集合差", ""]
        for k, title in (
            ("paper_missing_in_full", "卷面有、全自动无"),
            ("paper_missing_in_export", "卷面有、站外无"),
            ("full_not_in_paper", "全自动有、卷面未识别"),
            ("export_not_in_paper", "站外有、卷面未识别"),
        ):
            vals = cut.get(k) or []
            if vals:
                lines.append(f"- {title}：{', '.join(str(x) for x in vals)}")
        lines.append("")
    meta = data.get("meta") if isinstance(data.get("meta"), dict) else {}
    full_meta = meta.get("full") if isinstance(meta.get("full"), dict) else {}
    prog = full_meta.get("progress_summary") if isinstance(full_meta.get("progress_summary"), dict) else {}
    if prog:
        lines += [
            "## 附录（耗时，不计入对错）",
            "",
            f"- skip {prog.get('high_skip_n')}，llm_n {prog.get('llm_n')}，"
            f"markdown_to_json_ms {prog.get('markdown_to_json_ms')}",
            "",
        ]
    return "\n".join(lines)


def list_paper_dirs(root: Path) -> list[Path]:
    if (root / "paper.md").is_file():
        return [root]
    dirs = []
    for p in sorted(root.iterdir()):
        if (
            p.is_dir()
            and (p / "paper.md").is_file()
            and not p.name.startswith(("_", "."))
        ):
            dirs.append(p)
    return dirs


def self_check() -> int:
    assert normalize_question_no("１７（2）") == "17(2)"
    assert normalize_question_no("一、8") == "8"
    md = (
        "注意事项：1. 本试卷共 8 题\n\n"
        "1. 已知 $x=1$（ ）\nA. 1\nB. 2\nC. 3\nD. 4\n故选：B\n\n"
        "2. 求证（1）foo（2）bar\n【答案】（1）1（2）2\n\n"
        "19.已知 $a=1$ （1）求 a （2）求证 foo\n【答案】（1）1（2）略\n"
    )
    sl = slice_paper(md)
    assert "0:1" in sl and "0:2" in sl and "0:19" in sl
    assert paper_subq_count(sl["0:2"]) >= 2
    assert paper_subq_count(sl["0:19"]) >= 2
    restart_md = (
        "一、选择题\n"
        "1. 已知（ ）\nA. 1\nB. 2\nC. 3\nD. 4\n"
        "8. 已知（ ）\nA. 1\nB. 2\nC. 3\nD. 4\n"
        "二、填空题\n"
        "1. 则 m+n=\n【答案】-1\n"
    )
    restart = slice_paper(restart_md)
    assert "1:1" in restart and "1:8" in restart and "2:1" in restart
    assert "【答案】-1" in restart["2:1"]
    assert "A. 1" in restart["1:1"]
    parsed_ok = {
        "question_type": "choice",
        "stem": "已知 $x=1$（ ）",
        "options": [
            {"label": "A", "content": "1"},
            {"label": "B", "content": "2"},
            {"label": "C", "content": "3"},
            {"label": "D", "content": "4"},
        ],
        "correct_answer": {"kind": "choice", "value": {"options": ["B"]}},
        "analysis": [{"title": "解法一", "content": "因为 $x=1$，所以选 B。故选：B"}],
        "parts": [],
    }
    st, _ = score_choice_schema(parsed_ok, sl["0:1"])
    assert st == "pass", _
    st, rs = score_answer(parsed_ok, sl["0:1"])
    assert st == "pass", rs
    st, rs = score_editorial(parsed_ok)
    assert st == "pass", rs
    dirty = dict(parsed_ok)
    dirty["analysis"] = [{"title": "分析", "content": "根据定义即可判断."}]
    st, rs = score_editorial(dirty)
    assert st == "fail", rs
    long_strategy = dict(parsed_ok)
    long_strategy["analysis"] = [
        {
            "title": "解析",
            "content": "根据存在量词命题的否定为全称量词命题判断即可.\n\n命题为存在量词命题，其否定为全称。",
        }
    ]
    st, rs = score_editorial(long_strategy)
    assert st == "fail", rs
    single_nl = dict(parsed_ok)
    single_nl["analysis"] = [
        {
            "title": "解析",
            "content": "根据存在量词命题的否定为全称量词命题判断即可.\n命题为存在量词命题，后面还有很长的演算内容" + ("甲" * 80),
        }
    ]
    st, rs = score_editorial(single_nl)
    assert st == "fail", rs
    fill_q = {
        "question_type": "fill",
        "stem": "则 m+n=",
        "options": [],
        "correct_answer": {"kind": "fill", "value": {"blanks": [{"position": 1, "answer": "-1"}]}},
        "analysis": [],
        "parts": [],
    }
    fill_slice = "12. 则 m+n=\n【答案】-1\n"
    st, rs = score_answer(fill_q, fill_slice)
    assert st == "pass", rs
    st, rs = score_parts(fill_q, sl["0:19"])
    assert st == "na", rs
    sol = {
        "question_type": "solution",
        "stem": "已知 a=1",
        "correct_answer": {"kind": "solution", "value": {"subs": []}},
        "analysis": [{"title": "解析", "content": "略"}],
        "parts": [
            {"label": "(1)", "stem": "求 a", "answer": "1", "analyses": [], "children": []},
            {"label": "(2)", "stem": "求证", "answer": "略", "analyses": [], "children": []},
        ],
    }
    st, rs = score_parts(sol, sl["0:19"])
    assert st == "fail" and any("analysis" in x for x in rs), rs
    sol["analysis"] = []
    st, rs = score_parts(sol, sl["0:19"])
    assert st == "pass", rs
    leaf_one = {
        "question_type": "solution",
        "stem": "求证",
        "correct_answer": {"kind": "solution", "value": {"subs": []}},
        "analysis": [],
        "parts": [{"label": "(1)", "stem": "foo", "answer": "1", "analyses": [], "children": []}],
    }
    st, rs = score_answer(leaf_one, sl["0:2"])
    assert st == "fail", rs
    geo = "17. 设点 A. 为原点，集合 A={1}，点 B 在 x 轴上。\n【答案】略\n"
    assert not paper_is_choice(geo), "点 A / 集合 A 不得判选择题"
    print("self-check ok")
    return 0


def main() -> int:
    _stdio()
    parser = argparse.ArgumentParser(description="paper.md vs full.json vs export.json 规则分")
    parser.add_argument(
        "--dir",
        default=str(DEFAULT_EVAL),
        help="试卷目录或 bench/eval 根目录",
    )
    parser.add_argument("--self-check", action="store_true")
    args = parser.parse_args()
    if args.self_check:
        return self_check()

    root = Path(args.dir)
    if not root.is_absolute():
        root = (REPO_ROOT / root).resolve()
    dirs = list_paper_dirs(root)
    if not dirs:
        raise SystemExit(f"{root} 下没有含 paper.md 的试卷目录")

    reports: list[dict[str, Any]] = []
    failed_papers = 0
    for d in dirs:
        print(f"评测 {d.name} …")
        data = eval_paper(d)
        reports.append(data)
        (d / "report.json").write_text(
            json.dumps(data, ensure_ascii=False, indent=2) + "\n",
            encoding="utf-8",
        )
        md = write_report_md(data)
        (d / "report.md").write_text(md, encoding="utf-8")
        if data.get("import_failure"):
            print(f"  导入失败：{data['import_failure']}")
            failed_papers += 1
            continue
        sc = data.get("shunt_counts") or {}
        extra = " 无export" if data.get("export_absent") else ""
        print(
            f"  对齐 {data.get('aligned')}；"
            f"slice夹具 {sc.get('slice_fixture', 0)}，"
            f"站外未遵守 {sc.get('export_missed_prompt', 0)}，"
            f"两边同错 {sc.get('both_fail', 0)}，"
            f"未对齐 {sc.get('unaligned', 0)}{extra}"
        )
        print(f"  已写 {d / 'report.md'}")

    weighted: dict[str, dict[str, int]] = {
        b: {"full_fail": 0, "export_fail": 0, "both_fail": 0, "n": 0} for b in BUCKETS
    }
    total_questions = 0
    for r in reports:
        if r.get("import_failure"):
            continue
        n_q = int(r.get("full_count") or 0) or len(r.get("paper_question_nos") or [])
        total_questions += n_q
        rates = r.get("bucket_rates") or {}
        for b in BUCKETS:
            c = rates.get(b) or {}
            weighted[b]["full_fail"] += int(c.get("full_fail") or 0)
            weighted[b]["export_fail"] += int(c.get("export_fail") or 0)
            weighted[b]["both_fail"] += int(c.get("both_fail") or 0)
            weighted[b]["n"] += int(c.get("n") or 0)
    weighted_rates: dict[str, Any] = {}
    for b, c in weighted.items():
        n = c["n"] or 1
        weighted_rates[b] = {
            **c,
            "full_fail_rate": round(c["full_fail"] / n, 4) if c["n"] else None,
            "export_fail_rate": round(c["export_fail"] / n, 4) if c["n"] else None,
            "both_fail_rate": round(c["both_fail"] / n, 4) if c["n"] else None,
        }

    latest = (DEFAULT_EVAL if (root / "paper.md").is_file() else root) / "report_latest.json"
    latest.parent.mkdir(parents=True, exist_ok=True)
    latest.write_text(
        json.dumps(
            {
                "schema_version": "1",
                "rules": RULES_DOC,
                "evaluated_at": datetime.now(timezone.utc).isoformat(),
                "question_count": total_questions,
                "weighted_bucket_rates": weighted_rates,
                "papers": [
                    {
                        "name": r.get("name"),
                        "import_failure": r.get("import_failure"),
                        "export_absent": r.get("export_absent"),
                        "aligned": r.get("aligned"),
                        "full_count": r.get("full_count"),
                        "shunt_counts": r.get("shunt_counts"),
                        "bucket_rates": r.get("bucket_rates"),
                    }
                    for r in reports
                ],
            },
            ensure_ascii=False,
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )
    print(f"总清单 {latest}（按题加权 {total_questions} 题）")
    return 1 if failed_papers else 0


if __name__ == "__main__":
    sys.exit(main())
