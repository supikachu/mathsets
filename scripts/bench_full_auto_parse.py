#!/usr/bin/env python3
"""全自动解析压测：把 PDF 放进 bench/inbox，账号写进 bench/credentials.txt。

用法（仓库根目录）::

    python scripts/bench_full_auto_parse.py --label baseline

    python scripts/bench_full_auto_parse.py --label after --reuse bench/out/baseline_latest.json

    python scripts/bench_full_auto_parse.py --compare bench/out/baseline_latest.json bench/out/after_latest.json

    # 不新建解析任务：按上一轮 JSON 里的 task_id 拉取 MinerU 原文与全自动 JSON
    python scripts/bench_full_auto_parse.py --dump-from bench/out/after_latest.json

每份成功试卷会在 bench/eval/<试卷名>/ 落下：
  paper.md   MinerU OCR 原文（拿去跑站外模型）
  full.json  全自动 staged ParsedQuestion
  meta.json  任务 id、hash、耗时摘要

接口限制：POST /ai/documents 仍要求 pages 页图。本脚本附一张合法占位 PNG，
并把原 PDF 放在 pdf 字段，解析任务使用 pipeline=full + parse_mode=pdf_direct
（MinerU 整档直传，与界面全自动一致）。打标默认暂停，轮询不等待打标。

当前后端不回传上游 usage，token 列记为 n/a。
"""

from __future__ import annotations

import argparse
import binascii
import hashlib
import json
import re
import struct
import sys
import time
import urllib.error
import urllib.request
import uuid
import zlib
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_INBOX = REPO_ROOT / "bench" / "inbox"
DEFAULT_CREDS = REPO_ROOT / "bench" / "credentials.txt"
DEFAULT_OUT = REPO_ROOT / "bench" / "out"
DEFAULT_EVAL = REPO_ROOT / "bench" / "eval"
PROMPT_SOURCE = REPO_ROOT / "docs" / "rules-prompts.md"
PATCH_PROMPT_SOURCE = REPO_ROOT / "src" / "ai" / "prompt.rs"
WIN_RESERVED = re.compile(
    r"^(con|prn|aux|nul|com[1-9]|lpt[1-9])$", re.IGNORECASE
)
API_PREFIX = "/api/v1"
TERMINAL = frozenset(
    {"success", "partial_success", "failed", "cancelled", "completed"}
)
PDF_EXTS = {".pdf"}


def _configure_stdio() -> None:
    for stream in (sys.stdout, sys.stderr):
        reconf = getattr(stream, "reconfigure", None)
        if reconf is not None:
            try:
                reconf(encoding="utf-8")
            except Exception:
                pass


def placeholder_png(width: int = 16, height: int = 16) -> bytes:
    """合法 RGB PNG，边长 > 10px（上传接口视觉模型尺寸下限）。"""
    rows = bytearray()
    pixel = bytes((232, 232, 232))
    for _ in range(height):
        rows.append(0)
        rows.extend(pixel * width)

    def chunk(tag: bytes, data: bytes) -> bytes:
        crc = binascii.crc32(tag + data) & 0xFFFFFFFF
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", crc)

    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(rows), 9))
        + chunk(b"IEND", b"")
    )


PLACEHOLDER_PNG = placeholder_png()


def parse_credentials(path: Path) -> dict[str, str]:
    if not path.is_file():
        raise SystemExit(
            f"找不到账号文件：{path}\n"
            "请按 bench/credentials.example.txt 填写 username / password。"
        )
    data: dict[str, str] = {}
    text = path.read_text(encoding="utf-8-sig")
    stripped = text.strip()
    if stripped.startswith("{"):
        raw = json.loads(stripped)
        if not isinstance(raw, dict):
            raise SystemExit(f"{path} JSON 须为对象，含 username / password")
        data = {str(k).strip().lower(): str(v).strip() for k, v in raw.items()}
    else:
        for line in text.splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            if "=" in line:
                key, _, val = line.partition("=")
            elif ":" in line:
                key, _, val = line.partition(":")
            else:
                continue
            data[key.strip().lower()] = val.strip().strip('"').strip("'")
    username = data.get("username") or data.get("user") or ""
    password = data.get("password") or data.get("pass") or ""
    base_url = (
        data.get("base_url")
        or data.get("api")
        or data.get("url")
        or "http://127.0.0.1:3000"
    ).rstrip("/")
    if not username or not password:
        raise SystemExit(
            f"{path} 里 username / password 还是空的，填好后再跑。"
        )
    return {"username": username, "password": password, "base_url": base_url}


def list_pdfs(inbox: Path) -> list[Path]:
    if not inbox.is_dir():
        raise SystemExit(f"找不到试卷目录：{inbox}")
    files = sorted(
        p
        for p in inbox.iterdir()
        if p.is_file() and p.suffix.lower() in PDF_EXTS
    )
    return files


def parse_iso(value: Any) -> datetime | None:
    if not value or not isinstance(value, str):
        return None
    text = value.strip()
    if text.endswith("Z"):
        text = text[:-1] + "+00:00"
    try:
        dt = datetime.fromisoformat(text)
    except ValueError:
        return None
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return dt


def ms_between(start: Any, end: Any) -> int | None:
    a = parse_iso(start)
    b = parse_iso(end)
    if a is None or b is None:
        return None
    return max(0, int((b - a).total_seconds() * 1000))


def per_item(total: int | None, n: int) -> float | None:
    if total is None or n <= 0:
        return None
    return round(total / n, 1)


class ApiError(RuntimeError):
    def __init__(self, status: int, body: Any, url: str):
        self.status = status
        self.body = body
        self.url = url
        preview = body
        if isinstance(body, (dict, list)):
            preview = json.dumps(body, ensure_ascii=False)[:800]
        elif isinstance(body, (bytes, bytearray)):
            preview = bytes(body)[:400].decode("utf-8", "replace")
        super().__init__(f"HTTP {status} {url}: {preview}")


class ApiClient:
    def __init__(self, base_url: str, timeout: float = 120.0):
        self.base_url = base_url.rstrip("/")
        self.timeout = timeout
        self.token: str | None = None

    def _headers(self, extra: dict[str, str] | None = None) -> dict[str, str]:
        headers = {"Accept": "application/json"}
        if self.token:
            headers["Authorization"] = f"Bearer {self.token}"
        if extra:
            headers.update(extra)
        return headers

    def _url(self, path: str) -> str:
        if path.startswith("http"):
            return path
        if not path.startswith("/"):
            path = "/" + path
        return self.base_url + API_PREFIX + path

    def request(
        self,
        method: str,
        path: str,
        *,
        json_body: Any = None,
        data: bytes | None = None,
        headers: dict[str, str] | None = None,
        timeout: float | None = None,
    ) -> Any:
        url = self._url(path)
        extra = dict(headers or {})
        body = data
        if json_body is not None:
            body = json.dumps(json_body, ensure_ascii=False).encode("utf-8")
            extra["Content-Type"] = "application/json; charset=utf-8"
        req = urllib.request.Request(
            url,
            data=body,
            headers=self._headers(extra),
            method=method.upper(),
        )
        try:
            with urllib.request.urlopen(req, timeout=timeout or self.timeout) as resp:
                raw = resp.read()
                status = resp.status
        except urllib.error.HTTPError as e:
            raw = e.read()
            status = e.code
            parsed = _decode_json(raw)
            raise ApiError(status, parsed if parsed is not None else raw, url) from None
        except urllib.error.URLError as e:
            raise SystemExit(
                f"连不上后端 {self.base_url}（{e.reason}）。请确认服务已启动。"
            ) from None
        parsed = _decode_json(raw)
        if status >= 400:
            raise ApiError(status, parsed if parsed is not None else raw, url)
        return parsed

    def login(self, username: str, password: str) -> str:
        resp = self.request(
            "POST",
            "/auth/login",
            json_body={"username": username, "password": password},
            timeout=30,
        )
        if not isinstance(resp, dict) or not resp.get("token"):
            raise SystemExit(f"登录响应没有 token：{resp!r}")
        self.token = str(resp["token"])
        return self.token

    def upload_pdf(self, pdf_path: Path) -> dict[str, Any]:
        pdf_bytes = pdf_path.read_bytes()
        if not pdf_bytes.lstrip().startswith(b"%PDF"):
            raise SystemExit(f"不是 PDF（缺少 %PDF 头）：{pdf_path}")
        mp = MultipartForm()
        mp.add_file("pages", "page_1.png", PLACEHOLDER_PNG, "image/png")
        mp.add_file("pdf", pdf_path.name, pdf_bytes, "application/pdf")
        mp.add_field("file_name", pdf_path.name)
        mp.add_field("file_type", "pdf")
        resp = self.request(
            "POST",
            "/ai/documents",
            data=mp.body(),
            headers={"Content-Type": mp.content_type()},
            timeout=300,
        )
        doc = resp.get("data") if isinstance(resp, dict) else None
        if not isinstance(doc, dict) or not doc.get("id"):
            raise SystemExit(f"上传成功但响应没有 data.id：{resp!r}")
        return doc

    def create_parse_task(self, document_id: str) -> dict[str, Any]:
        resp = self.request(
            "POST",
            "/ai/parse-task",
            json_body={
                "document_id": document_id,
                "parse_mode": "pdf_direct",
                "pipeline": "full",
            },
            timeout=30,
        )
        if not isinstance(resp, dict) or not resp.get("task_id"):
            raise SystemExit(f"建任务响应没有 task_id：{resp!r}")
        return resp

    def get_task(self, task_id: str, timeout: float = 60) -> dict[str, Any]:
        resp = self.request("GET", f"/ai/parse-task/{task_id}", timeout=timeout)
        if not isinstance(resp, dict):
            raise SystemExit(f"任务状态不是 JSON 对象：{resp!r}")
        if isinstance(resp.get("data"), dict) and "status" in resp["data"]:
            inner = resp["data"]
            if isinstance(inner, dict):
                return inner
        return resp


def _decode_json(raw: bytes) -> Any:
    if not raw:
        return None
    try:
        return json.loads(raw.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError):
        return None


class MultipartForm:
    def __init__(self) -> None:
        self.boundary = "----MathsetBench" + uuid.uuid4().hex
        self._chunks: list[bytes] = []

    def add_field(self, name: str, value: str) -> None:
        self._chunks.append(
            (
                f"--{self.boundary}\r\n"
                f'Content-Disposition: form-data; name="{name}"\r\n\r\n'
                f"{value}\r\n"
            ).encode("utf-8")
        )

    def add_file(
        self, name: str, filename: str, data: bytes, content_type: str
    ) -> None:
        head = (
            f"--{self.boundary}\r\n"
            f'Content-Disposition: form-data; name="{name}"; '
            f'filename="{filename}"\r\n'
            f"Content-Type: {content_type}\r\n\r\n"
        ).encode("utf-8")
        self._chunks.append(head + data + b"\r\n")

    def content_type(self) -> str:
        return f"multipart/form-data; boundary={self.boundary}"

    def body(self) -> bytes:
        return b"".join(self._chunks) + f"--{self.boundary}--\r\n".encode("ascii")


def question_count(task: dict[str, Any]) -> int:
    timing = task.get("slice_timing") or {}
    for key in ("chunk_count",):
        n = timing.get(key) if isinstance(timing, dict) else None
        if isinstance(n, int) and n > 0:
            return n
    staged = task.get("staged_questions") or []
    if isinstance(staged, list) and staged:
        return len(staged)
    for key in ("success_count", "total_count", "processed_count"):
        n = task.get(key)
        if isinstance(n, int) and n > 0:
            return n
    return 0


def summarize_task(
    *,
    filename: str,
    document_id: str,
    task: dict[str, Any],
    client_wall_ms: int,
    reused: bool,
) -> dict[str, Any]:
    timing = task.get("slice_timing") if isinstance(task.get("slice_timing"), dict) else {}
    n_q = question_count(task)
    struct_ms = timing.get("markdown_to_json_ms")
    if not isinstance(struct_ms, int):
        struct_ms = None
    wall_ms = ms_between(task.get("started_at"), task.get("completed_at"))
    if wall_ms is None:
        wall_ms = client_wall_ms
    llm_n = timing.get("llm_n")
    high_skip_n = timing.get("high_skip_n")
    tokens = None
    for key in ("total_tokens", "prompt_tokens", "completion_tokens"):
        if isinstance(timing.get(key), int):
            tokens = {
                "prompt_tokens": timing.get("prompt_tokens"),
                "completion_tokens": timing.get("completion_tokens"),
                "total_tokens": timing.get("total_tokens"),
            }
            break
    denom_skip = None
    skip_rate = None
    if isinstance(llm_n, int) and isinstance(high_skip_n, int):
        denom_skip = llm_n + high_skip_n
        if denom_skip > 0:
            skip_rate = round(high_skip_n / denom_skip, 4)
    return {
        "filename": filename,
        "document_id": document_id,
        "task_id": task.get("id"),
        "status": task.get("status"),
        "error_message": task.get("error_message"),
        "reused_document": reused,
        "question_count": n_q,
        "success_count": task.get("success_count"),
        "failed_count": task.get("failed_count"),
        "processed_count": task.get("processed_count"),
        "client_wall_ms": client_wall_ms,
        "server_wall_ms": ms_between(task.get("started_at"), task.get("completed_at")),
        "markdown_to_json_ms": struct_ms,
        "ms_per_question_struct": per_item(struct_ms, n_q),
        "ms_per_question_wall": per_item(wall_ms, n_q),
        "llm_n": llm_n,
        "high_skip_n": high_skip_n,
        "skip_rate": skip_rate,
        "chunk_count": timing.get("chunk_count"),
        "split_ms": timing.get("split_ms"),
        "merge_recover_ms": timing.get("merge_recover_ms"),
        "split_via": timing.get("split_via"),
        "tagging_paused": timing.get("tagging_paused"),
        "tokens": tokens if tokens is not None else "n/a",
        "tokens_per_question": per_item(
            tokens.get("total_tokens") if isinstance(tokens, dict) else None, n_q
        ),
        "started_at": task.get("started_at"),
        "completed_at": task.get("completed_at"),
        "phase": task.get("phase"),
    }


def poll_until_done(
    client: ApiClient,
    task_id: str,
    *,
    timeout_sec: float,
    poll_sec: float,
) -> dict[str, Any]:
    deadline = time.monotonic() + timeout_sec
    last_line = ""
    while time.monotonic() < deadline:
        task = client.get_task(task_id)
        status = str(task.get("status") or "")
        phase = task.get("phase") or ""
        qno = task.get("current_question_no") or ""
        processed = task.get("processed_count")
        line = f"    {status} phase={phase} processed={processed} q={qno}"
        if line != last_line:
            print(line, flush=True)
            last_line = line
        if status in TERMINAL:
            return task
        time.sleep(poll_sec)
    raise SystemExit(f"任务 {task_id} 超过 {timeout_sec:.0f}s 仍未结束")


def load_reuse_map(path: Path) -> dict[str, dict[str, str]]:
    payload = json.loads(path.read_text(encoding="utf-8"))
    papers = payload.get("papers") if isinstance(payload, dict) else None
    if not isinstance(papers, list):
        raise SystemExit(f"{path} 不是压测结果（缺少 papers 数组）")
    mapping: dict[str, dict[str, str]] = {}
    for row in papers:
        if not isinstance(row, dict):
            continue
        name = row.get("filename")
        doc = row.get("document_id")
        if name and doc:
            rec: dict[str, str] = {"document_id": str(doc)}
            if row.get("task_id"):
                rec["task_id"] = str(row["task_id"])
            mapping[str(name)] = rec
    if not mapping:
        raise SystemExit(f"{path} 里没有 filename → document_id")
    return mapping


def mean(values: list[float]) -> float | None:
    if not values:
        return None
    return round(sum(values) / len(values), 1)


def aggregate(papers: list[dict[str, Any]]) -> dict[str, Any]:
    ok = [p for p in papers if p.get("status") in {"success", "partial_success", "completed"}]
    n_q = sum(int(p.get("question_count") or 0) for p in ok)
    struct_total = sum(
        int(p["markdown_to_json_ms"])
        for p in ok
        if isinstance(p.get("markdown_to_json_ms"), int)
    )
    wall_vals = [
        float(p["ms_per_question_wall"])
        for p in ok
        if isinstance(p.get("ms_per_question_wall"), (int, float))
    ]
    struct_vals = [
        float(p["ms_per_question_struct"])
        for p in ok
        if isinstance(p.get("ms_per_question_struct"), (int, float))
    ]
    llm_n = sum(int(p["llm_n"]) for p in ok if isinstance(p.get("llm_n"), int))
    high_skip_n = sum(
        int(p["high_skip_n"]) for p in ok if isinstance(p.get("high_skip_n"), int)
    )
    token_total = 0
    token_seen = False
    for p in ok:
        tok = p.get("tokens")
        if isinstance(tok, dict) and isinstance(tok.get("total_tokens"), int):
            token_total += tok["total_tokens"]
            token_seen = True
    skip_den = llm_n + high_skip_n
    return {
        "papers": len(papers),
        "ok_papers": len(ok),
        "question_count": n_q,
        "markdown_to_json_ms_sum": struct_total,
        "ms_per_question_struct_mean": per_item(struct_total, n_q) or mean(struct_vals),
        "ms_per_question_wall_mean": mean(wall_vals),
        "llm_n_sum": llm_n,
        "high_skip_n_sum": high_skip_n,
        "skip_rate": round(high_skip_n / skip_den, 4) if skip_den else None,
        "tokens_sum": token_total if token_seen else "n/a",
        "tokens_per_question": per_item(token_total if token_seen else None, n_q),
    }


def write_csv(path: Path, papers: list[dict[str, Any]]) -> None:
    cols = [
        "filename",
        "status",
        "question_count",
        "markdown_to_json_ms",
        "ms_per_question_struct",
        "ms_per_question_wall",
        "llm_n",
        "high_skip_n",
        "skip_rate",
        "tokens",
        "tokens_per_question",
        "task_id",
        "document_id",
    ]
    lines = [",".join(cols)]
    for p in papers:
        tok = p.get("tokens")
        tok_s = (
            str(tok.get("total_tokens"))
            if isinstance(tok, dict)
            else "n/a"
        )
        row = [
            str(p.get(c, "") if c != "tokens" else tok_s).replace(",", " ")
            for c in cols
        ]
        lines.append(",".join(row))
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def print_table(papers: list[dict[str, Any]], summary: dict[str, Any]) -> None:
    print()
    print(
        f"{'文件':<28} {'状态':<16} {'题数':>4} "
        f"{'结构ms/题':>10} {'端到端ms/题':>12} "
        f"{'LLM':>4} {'跳过':>4} {'token/题':>8}"
    )
    for p in papers:
        tok = p.get("tokens_per_question")
        tok_s = "n/a" if tok is None else str(tok)
        name = str(p.get("filename") or "")
        if len(name) > 26:
            name = name[:23] + "..."
        print(
            f"{name:<28} {str(p.get('status') or ''):<16} "
            f"{int(p.get('question_count') or 0):>4} "
            f"{_fmt(p.get('ms_per_question_struct')):>10} "
            f"{_fmt(p.get('ms_per_question_wall')):>12} "
            f"{_fmt(p.get('llm_n'), as_int=True):>4} "
            f"{_fmt(p.get('high_skip_n'), as_int=True):>4} "
            f"{tok_s:>8}"
        )
    print("-" * 96)
    print(
        f"合计 {summary['ok_papers']}/{summary['papers']} 份，"
        f"{summary['question_count']} 题；"
        f"结构 {summary['ms_per_question_struct_mean']} ms/题；"
        f"端到端 {summary['ms_per_question_wall_mean']} ms/题；"
        f"LLM {summary['llm_n_sum']}，跳过 {summary['high_skip_n_sum']}；"
        f"token {summary['tokens_sum']}"
    )


def _fmt(value: Any, as_int: bool = False) -> str:
    if value is None:
        return "-"
    if as_int and isinstance(value, (int, float)):
        return str(int(value))
    return str(value)


def paper_slug(filename: str) -> str:
    name = str(filename).replace("\\", "/").rstrip("/")
    if "/" in name:
        name = name.rsplit("/", 1)[-1]
    stem = name.rsplit(".", 1)[0] if "." in name else name
    stem = stem.strip() or "paper"
    cleaned = "".join("_" if ch in '<>:"/\\|?*' else ch for ch in stem).strip(" .")
    cleaned = cleaned or "paper"
    if WIN_RESERVED.match(cleaned):
        cleaned = f"_{cleaned}"
    return cleaned[:120]


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def relpath(path: Path) -> str:
    try:
        return path.resolve().relative_to(REPO_ROOT).as_posix()
    except ValueError:
        return str(path)


def load_locked_prompt() -> tuple[str | None, str | None]:
    if not PROMPT_SOURCE.is_file():
        return None, None
    text = PROMPT_SOURCE.read_text(encoding="utf-8")
    return text, sha256_text(text)


def load_stage2_patch_sha() -> tuple[str | None, str | None]:
    if not PATCH_PROMPT_SOURCE.is_file():
        return None, None
    text = PATCH_PROMPT_SOURCE.read_text(encoding="utf-8")
    marker = 'pub const STAGE2_PATCH_PROMPT: &str = r#"'
    start = text.find(marker)
    if start < 0:
        return None, None
    start += len(marker)
    end = text.find('"#;', start)
    if end < 0:
        return None, None
    body = text[start:end]
    return body, sha256_text(body)


def extract_full_questions(task: dict[str, Any]) -> list[dict[str, Any]]:
    staged = task.get("staged_questions") or []
    if not isinstance(staged, list):
        return []
    out: list[dict[str, Any]] = []
    for i, item in enumerate(staged):
        if not isinstance(item, dict):
            continue
        if item.get("merged_into"):
            continue
        parsed = item.get("parsed")
        if not isinstance(parsed, dict):
            parsed = {}
        order = item.get("order") if isinstance(item.get("order"), dict) else {}
        question_no = parsed.get("question_no") or order.get("question_no")
        display_order = parsed.get("display_order")
        if display_order is None:
            display_order = order.get("display_order")
        if display_order is None:
            display_order = i + 1
        rec: dict[str, Any] = {
            "question_no": question_no,
            "display_order": display_order,
            "index": item.get("index"),
            "parsed": parsed,
        }
        if "skip_or_llm" in item:
            rec["skip_or_llm"] = item.get("skip_or_llm")
        out.append(rec)
    return out


def dump_chunks_jsonl(
    paper_dir: Path,
    markdown: str,
    task: dict[str, Any],
    timing: dict[str, Any],
) -> None:
    """运行时没有切块时按评测再切，并标记 slice_source=eval_reparse。"""
    split_via = timing.get("split_via")
    rows: list[dict[str, Any]] = []
    runtime_chunks = task.get("chunks")
    if isinstance(runtime_chunks, list) and runtime_chunks:
        for i, ch in enumerate(runtime_chunks):
            if not isinstance(ch, dict):
                continue
            rows.append(
                {
                    "chunk_index": ch.get("chunk_index", i),
                    "source_md": ch.get("source_md") or "",
                    "split_via": ch.get("split_via") or split_via,
                    "bbox": ch.get("bbox"),
                    "slice_source": "runtime_chunk",
                }
            )
    else:
        slices: dict[str, str] = {}
        try:
            import importlib.util

            spec = importlib.util.spec_from_file_location(
                "bench_eval_quality",
                Path(__file__).with_name("bench_eval_quality.py"),
            )
            if spec and spec.loader:
                mod = importlib.util.module_from_spec(spec)
                spec.loader.exec_module(mod)
                slices = mod.slice_paper(markdown)
        except Exception:
            slices = {}
        if slices:
            for i, (key, md) in enumerate(slices.items()):
                rows.append(
                    {
                        "chunk_index": i,
                        "question_no": key,
                        "source_md": md,
                        "split_via": split_via,
                        "bbox": None,
                        "slice_source": "eval_reparse",
                    }
                )
        elif markdown.strip():
            rows.append(
                {
                    "chunk_index": 0,
                    "source_md": markdown,
                    "split_via": split_via,
                    "bbox": None,
                    "slice_source": "eval_reparse",
                }
            )
    path = paper_dir / "chunks.jsonl"
    path.write_text(
        "".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows),
        encoding="utf-8",
    )


def dump_eval_paper(
    eval_root: Path,
    *,
    filename: str,
    document_id: str | None,
    task: dict[str, Any],
    prompt_text: str | None,
    prompt_sha256: str | None,
    patch_sha256: str | None = None,
) -> dict[str, Any]:
    """把 MinerU 原文与全自动 JSON 写到 bench/eval/<试卷名>/。"""
    slug = paper_slug(filename)
    paper_dir = eval_root / slug
    if paper_dir.is_dir():
        meta_path = paper_dir / "meta.json"
        if meta_path.is_file():
            try:
                old = json.loads(meta_path.read_text(encoding="utf-8"))
            except (OSError, json.JSONDecodeError):
                old = {}
            old_doc = old.get("document_id")
            if old_doc and document_id and old_doc != document_id:
                paper_dir = eval_root / f"{slug}__{(document_id or '')[:8]}"
    paper_dir.mkdir(parents=True, exist_ok=True)

    markdown = task.get("ocr_markdown")
    if not isinstance(markdown, str):
        markdown = ""
    questions = extract_full_questions(task)
    timing = task.get("slice_timing") if isinstance(task.get("slice_timing"), dict) else {}
    md_sha = sha256_text(markdown) if markdown else None

    paper_md = paper_dir / "paper.md"
    full_json = paper_dir / "full.json"
    meta_json = paper_dir / "meta.json"
    export_json = paper_dir / "export.json"
    old_meta: dict[str, Any] = {}
    old_sha = None
    if meta_json.is_file():
        try:
            loaded = json.loads(meta_json.read_text(encoding="utf-8"))
            if isinstance(loaded, dict):
                old_meta = loaded
                ocr_old = loaded.get("ocr") if isinstance(loaded.get("ocr"), dict) else {}
                old_sha = ocr_old.get("markdown_sha256")
        except (OSError, json.JSONDecodeError):
            old_meta = {}

    stale_export = False
    kept_export = None
    if md_sha and old_sha and md_sha != old_sha and export_json.is_file():
        stale_path = paper_dir / "export.stale.json"
        export_json.replace(stale_path)
        stale_export = True
        warnings_pre = ["OCR markdown 已变，已将 export.json 改名为 export.stale.json"]
    else:
        warnings_pre = []
        if export_json.is_file() and (not md_sha or old_sha == md_sha):
            kept_export = old_meta.get("export")
            if kept_export is None:
                kept_export = {"path": "export.json"}

    paper_md.write_text(markdown, encoding="utf-8")
    full_payload = {
        "schema_version": "1",
        "pipeline": "full",
        "task_id": task.get("id"),
        "document_id": document_id or task.get("document_id"),
        "questions": questions,
    }
    full_json.write_text(
        json.dumps(full_payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    if prompt_text is not None:
        (eval_root / "prompt.md").write_text(prompt_text, encoding="utf-8")

    engine = task.get("ocr_engine")
    if not isinstance(engine, str) or not engine.strip():
        engine = None
    reused = bool(task.get("ocr_reused")) if "ocr_reused" in task else None
    meta = {
        "schema_version": "1",
        "document_id": document_id or task.get("document_id"),
        "source": {
            "filename": filename,
            "page_count": task.get("total_pages"),
        },
        "ocr": {
            "engine": engine,
            "markdown_sha256": md_sha,
            "chars": len(markdown),
            "path": "paper.md",
            "reused": reused,
        },
        "prompt": {
            "rules_prompts": {
                "source": "docs/rules-prompts.md",
                "sha256": prompt_sha256,
                "path": "../prompt.md",
            },
            "stage2_patch": {
                "source": "src/ai/prompt.rs STAGE2_PATCH_PROMPT",
                "sha256": patch_sha256,
            },
            "source": "docs/rules-prompts.md",
            "sha256": prompt_sha256,
            "path": "../prompt.md",
        },
        "full": {
            "task_id": task.get("id"),
            "pipeline": task.get("pipeline") or "full",
            "status": task.get("status"),
            "progress_summary": {
                "llm_n": timing.get("llm_n"),
                "high_skip_n": timing.get("high_skip_n"),
                "llm_calls": timing.get("llm_calls"),
                "markdown_to_json_ms": timing.get("markdown_to_json_ms"),
                "chunk_count": timing.get("chunk_count"),
                "split_via": timing.get("split_via"),
            },
            "questions_path": "full.json",
            "question_count": len(questions),
        },
        "export": kept_export if not stale_export else None,
        "next": "把 paper.md 与 ../prompt.md 交给站外模型，将 {\"questions\":[...]} 存为同目录 export.json",
    }
    dump_chunks_jsonl(paper_dir, markdown, task, timing)
    meta_json.write_text(
        json.dumps(meta, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    warnings: list[str] = list(warnings_pre)
    if not markdown.strip():
        warnings.append("ocr_markdown 为空")
    if not questions:
        warnings.append("staged_questions 为空")
    return {
        "filename": filename,
        "slug": paper_dir.name,
        "dir": relpath(paper_dir),
        "document_id": document_id or task.get("document_id"),
        "task_id": task.get("id"),
        "markdown_sha256": md_sha,
        "markdown_chars": len(markdown),
        "question_count": len(questions),
        "files": {
            "paper_md": relpath(paper_md),
            "full_json": relpath(full_json),
            "meta_json": relpath(meta_json),
        },
        "warnings": warnings,
    }


def write_eval_manifest(
    eval_root: Path,
    records: list[dict[str, Any]],
    prompt_sha256: str | None,
) -> Path:
    payload = {
        "schema_version": "1",
        "dumped_at": datetime.now(timezone.utc).isoformat(),
        "prompt_source": "docs/rules-prompts.md",
        "prompt_sha256": prompt_sha256,
        "papers": records,
    }
    path = eval_root / "manifest.json"
    path.write_text(
        json.dumps(payload, ensure_ascii=False, indent=2) + "\n",
        encoding="utf-8",
    )
    return path


def dump_from_existing(args: argparse.Namespace) -> int:
    creds = parse_credentials(Path(args.credentials))
    src = Path(args.dump_from)
    if not src.is_file():
        raise SystemExit(f"找不到上一轮结果：{src}")
    payload = json.loads(src.read_text(encoding="utf-8"))
    papers = payload.get("papers") if isinstance(payload, dict) else None
    if not isinstance(papers, list) or not papers:
        raise SystemExit(f"{src} 里没有 papers")

    eval_root = Path(args.eval_dir)
    eval_root.mkdir(parents=True, exist_ok=True)
    prompt_text, prompt_sha = load_locked_prompt()
    _, patch_sha = load_stage2_patch_sha()
    if prompt_text is None:
        print("警告：找不到 docs/rules-prompts.md，未写入 prompt.md")
    else:
        (eval_root / "prompt.md").write_text(prompt_text, encoding="utf-8")

    client = ApiClient(creds["base_url"])
    print(f"登录 {creds['base_url']} 用户 {creds['username']} …")
    client.login(creds["username"], creds["password"])
    print(f"从 {src} 拉取 {len(papers)} 份任务的 OCR / 全自动 JSON → {relpath(eval_root)}")

    records: list[dict[str, Any]] = []
    failed = 0
    for i, row in enumerate(papers, 1):
        if not isinstance(row, dict):
            continue
        filename = str(row.get("filename") or f"paper_{i}")
        task_id = row.get("task_id")
        document_id = row.get("document_id")
        print(f"\n[{i}/{len(papers)}] {filename}")
        if not task_id:
            print("  跳过：没有 task_id")
            failed += 1
            continue
        try:
            task = client.get_task(str(task_id), timeout=120)
        except ApiError as e:
            print(f"  拉取失败：{e}")
            failed += 1
            continue
        rec = dump_eval_paper(
            eval_root,
            filename=filename,
            document_id=str(document_id) if document_id else None,
            task=task,
            prompt_text=prompt_text,
            prompt_sha256=prompt_sha,
            patch_sha256=patch_sha,
        )
        records.append(rec)
        warn = f"（{'; '.join(rec['warnings'])}）" if rec["warnings"] else ""
        print(
            f"  已写入 {rec['dir']}  "
            f"markdown={rec['markdown_chars']}字 题数={rec['question_count']}{warn}"
        )

    manifest = write_eval_manifest(eval_root, records, prompt_sha)
    print(f"\n清单 {relpath(manifest)}，成功 {len(records)}/{len(papers)}")
    if records:
        print("把各目录下的 paper.md 与 bench/eval/prompt.md 交给站外模型；")
        print("返回的 {\"questions\":[...]} 存为同目录 export.json（下一步）。")
    return 1 if failed else 0


def compare_runs(a_path: Path, b_path: Path) -> int:
    a = json.loads(a_path.read_text(encoding="utf-8"))
    b = json.loads(b_path.read_text(encoding="utf-8"))
    sa, sb = a.get("summary") or {}, b.get("summary") or {}
    keys = [
        ("question_count", "题数"),
        ("ms_per_question_struct_mean", "结构 ms/题"),
        ("ms_per_question_wall_mean", "端到端 ms/题"),
        ("llm_n_sum", "LLM 次数"),
        ("high_skip_n_sum", "高置信跳过"),
        ("skip_rate", "跳过率"),
        ("tokens_sum", "token 合计"),
        ("tokens_per_question", "token/题"),
    ]
    print(f"对比 {a.get('label')} → {b.get('label')}")
    print(f"{'指标':<16} {str(a.get('label')):>14} {str(b.get('label')):>14} {'变化':>12}")
    for key, title in keys:
        va, vb = sa.get(key), sb.get(key)
        delta = ""
        if isinstance(va, (int, float)) and isinstance(vb, (int, float)):
            if va:
                pct = (vb - va) / va * 100
                delta = f"{vb - va:+.1f} ({pct:+.1f}%)"
            else:
                delta = f"{vb - va:+.1f}"
        print(f"{title:<16} {_fmt(va):>14} {_fmt(vb):>14} {delta:>12}")
    return 0


def run_bench(args: argparse.Namespace) -> int:
    creds = parse_credentials(Path(args.credentials))
    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    inbox = Path(args.inbox)
    pdfs = list_pdfs(inbox)
    reuse_map: dict[str, dict[str, str]] = {}
    if args.reuse:
        reuse_map = load_reuse_map(Path(args.reuse))
        print(f"复用上一轮 document_id：{Path(args.reuse)}（{len(reuse_map)} 份）；无 OCR 缓存则失败退出")

    jobs: list[tuple[str, Path | None, dict[str, str] | None]] = []
    if pdfs:
        for path in pdfs:
            jobs.append((path.name, path, reuse_map.get(path.name)))
    elif reuse_map:
        for name, rec in reuse_map.items():
            jobs.append((name, None, rec))
    else:
        raise SystemExit(
            f"{inbox} 里没有 PDF。\n"
            "把待测试卷放进该文件夹后再跑；优化后复测可用 --reuse 指向上一轮 JSON。"
        )
    if args.reuse:
        missing = [name for name, _, rec in jobs if not rec or not rec.get("document_id")]
        if missing:
            raise SystemExit(
                f"--reuse 缺少 document_id：{', '.join(missing)}。禁止默默重跑 OCR。"
            )

    client = ApiClient(creds["base_url"])
    print(f"登录 {creds['base_url']} 用户 {creds['username']} …")
    client.login(creds["username"], creds["password"])
    print(f"开始全自动压测 {len(jobs)} 份，label={args.label}")

    eval_root = Path(args.eval_dir)
    eval_root.mkdir(parents=True, exist_ok=True)
    prompt_text, prompt_sha = load_locked_prompt()
    _, patch_sha = load_stage2_patch_sha()
    if prompt_text is None:
        print("警告：找不到 docs/rules-prompts.md，未写入 prompt.md")
    else:
        (eval_root / "prompt.md").write_text(prompt_text, encoding="utf-8")

    papers: list[dict[str, Any]] = []
    eval_records: list[dict[str, Any]] = []
    for i, (name, path, reuse_rec) in enumerate(jobs, 1):
        print(f"\n[{i}/{len(jobs)}] {name}")
        reused = False
        reused_doc: str | None = None
        try:
            if reuse_rec:
                document_id = reuse_rec["document_id"]
                reused_doc = document_id
                reused = True
                prev_task_id = reuse_rec.get("task_id")
                if not prev_task_id:
                    raise SystemExit(
                        f"{name} 上一轮没有 task_id，无法确认 OCR 缓存。禁止 --reuse 默默重跑 OCR。"
                    )
                prev_task = client.get_task(prev_task_id, timeout=120)
                prev_md = prev_task.get("ocr_markdown")
                if not isinstance(prev_md, str) or not prev_md.strip():
                    raise SystemExit(
                        f"{name} 上一轮任务 {prev_task_id} 没有 ocr_markdown，"
                        "禁止 --reuse 默默重跑 OCR。请改用 --dump-from。"
                    )
                print(
                    f"  复用 document_id={document_id}；"
                    f"已确认 OCR 缓存 {len(prev_md)} 字，将跳过 MinerU"
                )
            else:
                assert path is not None
                print("  上传 PDF（占位页图 + 原文件，pdf_direct）…")
                doc = client.upload_pdf(path)
                document_id = str(doc["id"])
                print(f"  document_id={document_id}")
                prev_md = None
            t0 = time.monotonic()
            created = client.create_parse_task(document_id)
            task_id = str(created["task_id"])
            print(f"  task_id={task_id} 等待结束…")
            task = poll_until_done(
                client,
                task_id,
                timeout_sec=args.timeout_sec,
                poll_sec=args.poll_sec,
            )
            client_wall_ms = int((time.monotonic() - t0) * 1000)
            new_md = task.get("ocr_markdown") if isinstance(task.get("ocr_markdown"), str) else ""
            if reused and isinstance(prev_md, str):
                if sha256_text(new_md or "") == sha256_text(prev_md):
                    print("  跳过 OCR（markdown sha 与上一轮一致）")
                else:
                    print("  警告：--reuse 后 markdown sha 变化，可能仍跑了 OCR")
            if task.get("ocr_reused") is True:
                print("  后端标记 ocr_reused=true")
            row = summarize_task(
                filename=name,
                document_id=document_id,
                task=task,
                client_wall_ms=client_wall_ms,
                reused=reused,
            )
            rec = dump_eval_paper(
                eval_root,
                filename=name,
                document_id=document_id,
                task=task,
                prompt_text=prompt_text,
                prompt_sha256=prompt_sha,
                patch_sha256=patch_sha,
            )
            eval_records.append(rec)
            row["eval_dir"] = rec["dir"]
            row["eval_files"] = rec["files"]
            row["ocr_reused"] = bool(task.get("ocr_reused"))
            papers.append(row)
            warn = f" {' '.join(rec['warnings'])}" if rec["warnings"] else ""
            print(
                f"  完成 status={row['status']} 题数={row['question_count']} "
                f"结构={row['ms_per_question_struct']}ms/题 "
                f"LLM={row['llm_n']} 跳过={row['high_skip_n']}"
            )
            print(
                f"  已导出 {rec['dir']}/paper.md 与 full.json"
                f"（{rec['markdown_chars']} 字 / {rec['question_count']} 题）{warn}"
            )
        except SystemExit:
            raise
        except ApiError as e:
            err = {
                "filename": name,
                "document_id": reused_doc,
                "status": "error",
                "error_message": str(e),
                "reused_document": reused,
                "question_count": 0,
                "tokens": "n/a",
            }
            papers.append(err)
            print(f"  失败：{e}")
            if e.status == 403:
                print("  今日解析任务额度可能已用尽，停止后续试卷。")
                break

    summary = aggregate(papers)
    stamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    payload = {
        "label": args.label,
        "created_at": datetime.now(timezone.utc).isoformat(),
        "base_url": creds["base_url"],
        "pipeline": "full",
        "parse_mode": "pdf_direct",
        "note": "token 列 n/a：后端尚未回传上游 usage",
        "papers": papers,
        "summary": summary,
    }
    latest = out_dir / f"{args.label}_latest.json"
    stamped = out_dir / f"{args.label}_{stamp}.json"
    csv_path = out_dir / f"{args.label}_latest.csv"
    text = json.dumps(payload, ensure_ascii=False, indent=2)
    stamped.write_text(text, encoding="utf-8")
    latest.write_text(text, encoding="utf-8")
    write_csv(csv_path, papers)
    print_table(papers, summary)
    print(f"\n已写入 {latest}")
    print(f"已写入 {stamped}")
    print(f"已写入 {csv_path}")
    if eval_records:
        manifest = write_eval_manifest(eval_root, eval_records, prompt_sha)
        print(f"评测原文/JSON 已写入 {relpath(eval_root)} （{len(eval_records)} 份）")
        print(f"清单 {relpath(manifest)}")
    failed = [p for p in papers if p.get("status") not in {"success", "partial_success", "completed"}]
    return 1 if failed else 0


def main() -> int:
    _configure_stdio()
    parser = argparse.ArgumentParser(
        description="全自动解析压测：inbox 放 PDF，credentials.txt 写账号密码"
    )
    parser.add_argument("--inbox", default=str(DEFAULT_INBOX), help="待测 PDF 目录")
    parser.add_argument(
        "--credentials",
        default=str(DEFAULT_CREDS),
        help="账号文件（username= / password=）",
    )
    parser.add_argument("--out", default=str(DEFAULT_OUT), help="结果输出目录")
    parser.add_argument("--label", default="baseline", help="本轮标签，如 baseline / after")
    parser.add_argument(
        "--reuse",
        default="",
        help="上一轮结果 JSON：按文件名复用 document_id；须已有 ocr_markdown，否则失败退出（不重跑 OCR）",
    )
    parser.add_argument(
        "--timeout-sec",
        type=float,
        default=3600,
        help="每份试卷最长等待秒数（默认 3600）",
    )
    parser.add_argument("--poll-sec", type=float, default=3.0, help="轮询间隔秒")
    parser.add_argument(
        "--eval-dir",
        default=str(DEFAULT_EVAL),
        help="MinerU 原文与全自动 JSON 落盘目录（默认 bench/eval）",
    )
    parser.add_argument(
        "--dump-from",
        default="",
        help="上一轮压测 JSON：按 task_id 拉取 OCR/全自动 JSON 落盘，不新建解析任务",
    )
    parser.add_argument(
        "--compare",
        nargs=2,
        metavar=("BEFORE", "AFTER"),
        help="对比两轮 JSON，不跑解析",
    )
    args = parser.parse_args()
    if args.compare:
        return compare_runs(Path(args.compare[0]), Path(args.compare[1]))
    if args.dump_from:
        return dump_from_existing(args)
    return run_bench(args)


if __name__ == "__main__":
    sys.exit(main())
