-- =============================================================================
-- 用户中台与协同权限体系 (User Hub & RBAC)
-- 身份双轨制 + 空间角色枚举化 + 用户头像与 OCR 额度
-- =============================================================================

-- ┌───────────────────────────────────────────────────────────────────────────┐
-- │ 1) 全局角色枚举 (Global Role)                                            │
-- │    super_admin = 全局系统管理员                                           │
-- │    teacher     = 普通教师                                                │
-- └───────────────────────────────────────────────────────────────────────────┘

DO $$ BEGIN
    CREATE TYPE global_role AS ENUM ('super_admin', 'teacher');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

ALTER TABLE users ADD COLUMN IF NOT EXISTS global_role global_role NOT NULL DEFAULT 'teacher';

-- 兼容迁移：将原 user_role.admin 映射为 super_admin
UPDATE users SET global_role = 'super_admin' WHERE role::text IN ('admin', 'Admin');
UPDATE users SET global_role = 'teacher'     WHERE role::text NOT IN ('admin', 'Admin');

CREATE INDEX IF NOT EXISTS idx_users_global_role ON users(global_role);

-- ┌───────────────────────────────────────────────────────────────────────────┐
-- │ 2) 用户扩展字段：头像 + OCR/AI 解析额度                                   │
-- └───────────────────────────────────────────────────────────────────────────┘

ALTER TABLE users ADD COLUMN IF NOT EXISTS avatar_url VARCHAR(500);

-- OCR 识别日额度
ALTER TABLE users ADD COLUMN IF NOT EXISTS ocr_quota_daily    INT NOT NULL DEFAULT 50;
ALTER TABLE users ADD COLUMN IF NOT EXISTS ocr_quota_used     INT NOT NULL DEFAULT 0;
ALTER TABLE users ADD COLUMN IF NOT EXISTS ocr_quota_reset_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

-- AI 智能解析 Token 额度
ALTER TABLE users ADD COLUMN IF NOT EXISTS ai_token_quota     INT NOT NULL DEFAULT 100000;

CREATE INDEX IF NOT EXISTS idx_users_active_lookup ON users(is_active, created_at DESC);

-- ┌───────────────────────────────────────────────────────────────────────────┐
-- │ 3) 空间角色枚举 (Space Role)                                             │
-- │    owner    = 空间管理员                                                 │
-- │    editor   = 录题员（仅 Draft/Pending）                                  │
-- │    reviewer = 审题员（Pending -> Published/Rejected）                      │
-- │    viewer   = 观察员（只读）                                              │
-- └───────────────────────────────────────────────────────────────────────────┘

DO $$ BEGIN
    CREATE TYPE space_role AS ENUM ('owner', 'editor', 'reviewer', 'viewer');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- ┌───────────────────────────────────────────────────────────────────────────┐
-- │ 4) 将 space_members.role 从 VARCHAR(20) 迁移为 space_role ENUM           │
-- │    已有值映射：'owner' -> owner, 其余 -> editor                           │
-- └───────────────────────────────────────────────────────────────────────────┘

-- 仅当 role 列仍为 varchar 时执行迁移
DO $$
DECLARE
    col_type TEXT;
BEGIN
    SELECT data_type INTO col_type
    FROM information_schema.columns
    WHERE table_name = 'space_members' AND column_name = 'role';

    IF col_type = 'character varying' THEN
        -- 1. 先删掉旧的默认值，防止 42804 类型冲突
        ALTER TABLE space_members ALTER COLUMN role DROP DEFAULT;

        -- 2. 转换类型（显式 ::text 比对避免类型歧义）
        ALTER TABLE space_members
            ALTER COLUMN role TYPE space_role
            USING (
                CASE
                    WHEN role::text IN ('owner', 'Owner')       THEN 'owner'::space_role
                    WHEN role::text IN ('reviewer', 'Reviewer') THEN 'reviewer'::space_role
                    WHEN role::text IN ('viewer', 'Viewer')     THEN 'viewer'::space_role
                    ELSE 'editor'::space_role
                END
            );

        -- 3. 重新绑定新的枚举默认值
        ALTER TABLE space_members ALTER COLUMN role SET DEFAULT 'editor'::space_role;
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_space_members_role ON space_members(space_id, role);

-- ┌───────────────────────────────────────────────────────────────────────────┐
-- │ 5) 空间表新增设置字段注释（描述业务语义）                                   │
-- └───────────────────────────────────────────────────────────────────────────┘

COMMENT ON COLUMN spaces.settings IS '空间审核规则 JSONB:
  allow_creator_self_review (bool) — 个人空间：允许自审
  require_review_duty (bool) — 团队空间：需要具有 review 职责
  enforce_maker_checker (bool, 默认 true) — 团队空间：强制录审分离';
