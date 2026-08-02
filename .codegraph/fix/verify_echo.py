# -*- coding: utf-8 -*-
"""端到端回显验证：GET /questions/:id 必须返回完整 tags + knowledge_nodes"""
import json
import urllib.request

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
            return r.status, json.loads(r.read().decode())
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read().decode())


code, body = call("/api/v1/auth/login", "POST",
                  {"username": "visualtest", "password": "Test123456"})
assert code == 200, f"login failed: {code} {body}"
token = body.get("token") or body.get("access_token")
assert token, "no token"

ok = True
for qid, min_tags, min_kns in [
    ("d7b71b6b-9b22-4b8e-8b8c-42c5dccb3da5", 1, 1),
    ("32b9c3af-aac2-4a37-ab52-1873bb674b6d", 1, 1),
]:
    code, d = call(f"/api/v1/questions/{qid}", token=token)
    assert code == 200, f"GET {qid} -> {code}"
    tags = d.get("tags") or []
    kns = d.get("knowledge_nodes") or []
    cats = sorted({t.get("category") for t in tags})
    print(f"GET /questions/{qid}: tags={len(tags)} (categories={cats}), "
          f"knowledge_nodes={len(kns)}")
    if len(tags) < min_tags or len(kns) < min_kns:
        ok = False
    for t in tags:
        assert t.get("id") and t.get("name") and t.get("category"), "tag 字段缺失"
    for k in kns:
        assert k.get("id") and k.get("tree_id") and k.get("name"), "node 字段缺失"

print("RESULT:", "PASS - 回显链路完整" if ok else "FAIL")
