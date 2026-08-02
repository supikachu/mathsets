# -*- coding: utf-8 -*-
"""复现 create_question 422 错误：证明 cargo test 失败源于旧测试难度格式"""
import json
import urllib.request
import uuid

BASE = "http://127.0.0.1:3000"


def call(path, method="GET", body=None, token=None):
    req = urllib.request.Request(
        BASE + path, method=method,
        data=json.dumps(body).encode() if body is not None else None,
        headers={"Content-Type": "application/json"},
    )
    if token:
        req.add_header("Authorization", "Bearer " + token)
    try:
        with urllib.request.urlopen(req, timeout=10) as r:
            raw = r.read()
            return r.status, (json.loads(raw.decode()) if raw else None)
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw.decode())
        except Exception:
            return e.code, raw.decode(errors="replace")[:300]


uname = "probe_" + str(uuid.uuid4())[:8]
call("/api/v1/auth/register", "POST", {
    "username": uname, "email": f"{uname}@test.com",
    "password": "test123", "display_name": "探测用户"})
_, b2 = call("/api/v1/auth/login", "POST", {"username": uname, "password": "test123"})
token = (b2 or {}).get("token")

# 模拟 tests/api.rs:817-825 的旧格式（difficulty: "easy" 字符串）
code, body = call("/api/v1/questions", "POST", {
    "stem": "探测题", "question_type": "choice", "difficulty": "easy",
    "correct_answer": ["A"],
    "options": [{"label": "A", "content": "正确"}, {"label": "B", "content": "错误"}],
}, token=token)
print(f"旧格式 difficulty='easy' -> HTTP {code}")
print(f"错误: {body if isinstance(body, str) else json.dumps(body, ensure_ascii=False)}")

# 对照：新格式 difficulty=1（i16）
code2, body2 = call("/api/v1/questions", "POST", {
    "stem": "探测题2", "question_type": "choice", "difficulty": 1,
    "correct_answer": ["A"],
    "options": [{"label": "A", "content": "正确"}, {"label": "B", "content": "错误"}],
}, token=token)
qid = body2.get("id") if isinstance(body2, dict) else None
print(f"新格式 difficulty=1 -> HTTP {code2}, question_id={qid}")
print("RESULT:", "定位确认 - 测试失败源于旧 difficulty 字符串格式，非服务缺陷" if code == 422 and qid else "未知")
