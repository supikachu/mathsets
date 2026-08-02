# -*- coding: utf-8 -*-
"""任务 B 验证：super_admin 可访问他人题目，普通用户仍被 403 拦截"""
import json
import urllib.request
import uuid

BASE = "http://127.0.0.1:3000"
TARGET_QID = "d7b71b6b-9b22-4b8e-8b8c-42c5dccb3da5"  # visualtest 个人空间的题


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
        raw = e.read()
        try:
            return e.code, json.loads(raw.decode())
        except Exception:
            return e.code, raw.decode(errors="replace")[:200]


# 1) super_admin（旧 role=user，双轨制典型场景）访问他人题目
code, body = call("/api/v1/auth/login", "POST",
                  {"username": "leader_92917ce8", "password": "test123"})
token = (body or {}).get("token")
assert token, f"leader 登录失败: {body}"
print(f"[1] leader(super_admin) 登录: OK, global_role={body.get('global_role')}, role={body.get('role')}")

s1, b1 = call(f"/api/v1/questions/{TARGET_QID}", token=token)
print(f"[2] super_admin GET 他人题目 -> HTTP {s1}")
assert s1 == 200, f"super_admin 应能访问他人题目（修复前 403），实际 {s1}: {b1}"

# 2) 普通新用户访问同一题目 → 必须仍 403
uname = "perm_probe_" + str(uuid.uuid4())[:8]
call("/api/v1/auth/register", "POST", {
    "username": uname, "email": f"{uname}@test.com",
    "password": "test123", "display_name": "权限探测"})
_, b2 = call("/api/v1/auth/login", "POST", {"username": uname, "password": "test123"})
t2 = (b2 or {}).get("token")
s2, _ = call(f"/api/v1/questions/{TARGET_QID}", token=t2)
print(f"[3] 普通用户 GET 他人题目 -> HTTP {s2}")
assert s2 == 403, f"普通用户访问他人题目应 403，实际 {s2}"

print("RESULT: PASS - super_admin 不再被误拦截，普通用户越权拦截保持")
