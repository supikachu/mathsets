# -*- coding: utf-8 -*-
"""任务 B：双轨制权限漏洞修复 — is_admin(&auth.role) → is_admin_user(&auth)"""
import io
import re

FILES = [
    "src/handlers/questions.rs",
    "src/handlers/papers.rs",
    "src/handlers/public_library.rs",
    "src/handlers/spaces.rs",
    "src/handlers/tags.rs",
    "src/handlers/ai_tasks.rs",
    "src/auth/permissions.rs",
]

total = 0
for path in FILES:
    src = io.open(path, encoding="utf-8").read()
    before = src
    # 调用点替换（两种变量名）
    src = src.replace("is_admin(&auth.role)", "is_admin_user(&auth)")
    src = src.replace("is_admin(&auth_user.role)", "is_admin_user(&auth_user)")
    n = before.count("is_admin(&auth.role)") + before.count("is_admin(&auth_user.role)")
    total += n
    if src != before:
        io.open(path, "w", encoding="utf-8").write(src)
    print(f"  {path}: 替换 {n} 处")

# import 更新
imports = {
    "src/handlers/questions.rs": (
        "    can_access_space, can_edit_question, can_publish_question, can_review_question,\n"
        "    can_write_in_space, ensure_personal_space, ensure_public_space, get_member_meta, get_space,\n"
        "    is_admin, list_reviewers, PermissionError,\n",
        "    can_access_space, can_edit_question, can_publish_question, can_review_question,\n"
        "    can_write_in_space, ensure_personal_space, ensure_public_space, get_member_meta, get_space,\n"
        "    is_admin_user, list_reviewers, PermissionError,\n",
    ),
    "src/handlers/papers.rs": (
        "use crate::auth::permissions::is_admin;\n",
        "use crate::auth::permissions::is_admin_user;\n",
    ),
    "src/handlers/public_library.rs": (
        "use crate::auth::permissions::{ensure_public_space, get_space, is_admin};\n",
        "use crate::auth::permissions::{ensure_public_space, get_space, is_admin_user};\n",
    ),
    "src/handlers/spaces.rs": (
        "    can_access_space, ensure_personal_space, ensure_public_space, get_space, is_admin,\n"
        "    is_space_member,\n",
        "    can_access_space, ensure_personal_space, ensure_public_space, get_space, is_admin_user,\n"
        "    is_space_member,\n",
    ),
    "src/handlers/tags.rs": (
        "use crate::auth::permissions::is_admin;\n",
        "use crate::auth::permissions::is_admin_user;\n",
    ),
    "src/handlers/ai_tasks.rs": (
        "use crate::auth::permissions::is_admin;\n",
        "use crate::auth::permissions::is_admin_user;\n",
    ),
}
for path, (old_imp, new_imp) in imports.items():
    src = io.open(path, encoding="utf-8").read()
    if old_imp in src:
        src = src.replace(old_imp, new_imp)
        io.open(path, "w", encoding="utf-8").write(src)
        print(f"  {path}: import 更新")
    else:
        print(f"  {path}: import 未匹配（检查）")

print(f"总计替换调用点 {total} 处")
