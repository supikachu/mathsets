-- =============================================================================
-- 题库空间 + 角色简化 + 指定审题人
-- =============================================================================

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- 1) 空间类型
DO $$ BEGIN
    CREATE TYPE space_kind AS ENUM ('personal', 'team', 'public');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- 2) 空间表
CREATE TABLE IF NOT EXISTS spaces (
    id              UUID PRIMARY KEY,
    kind            space_kind NOT NULL,
    name            VARCHAR(100) NOT NULL,
    owner_user_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    settings        JSONB NOT NULL DEFAULT '{
        "allow_creator_self_review": true,
        "require_review_duty": false
    }'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_spaces_kind ON spaces(kind);
CREATE INDEX IF NOT EXISTS idx_spaces_owner ON spaces(owner_user_id);

-- 个人空间：每个用户最多一个
CREATE UNIQUE INDEX IF NOT EXISTS idx_spaces_one_personal_per_user
    ON spaces (owner_user_id)
    WHERE kind = 'personal';

-- 公共空间：全站唯一
CREATE UNIQUE INDEX IF NOT EXISTS idx_spaces_one_public
    ON spaces (kind)
    WHERE kind = 'public';

-- 3) 空间成员（团队为主；个人/公共可不写）
CREATE TABLE IF NOT EXISTS space_members (
    space_id        UUID NOT NULL REFERENCES spaces(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    role            VARCHAR(20) NOT NULL DEFAULT 'member',  -- owner | member
    duties          TEXT[] NOT NULL DEFAULT '{}',            -- entry | review | analysis
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (space_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_space_members_user ON space_members(user_id);

-- 4) 初始化公共空间
INSERT INTO spaces (id, kind, name, owner_user_id, settings, created_at, updated_at)
SELECT
    '00000000-0000-0000-0000-000000000001'::uuid,
    'public',
    '公共题库',
    NULL,
    '{"allow_creator_self_review": false, "require_review_duty": false}'::jsonb,
    NOW(),
    NOW()
WHERE NOT EXISTS (SELECT 1 FROM spaces WHERE kind = 'public');

-- 5) 为已有用户创建个人空间
INSERT INTO spaces (id, kind, name, owner_user_id, settings, created_at, updated_at)
SELECT
    gen_random_uuid(),
    'personal',
    COALESCE(u.display_name, u.username) || ' 的题库',
    u.id,
    '{"allow_creator_self_review": true, "require_review_duty": false}'::jsonb,
    NOW(),
    NOW()
FROM users u
WHERE NOT EXISTS (
    SELECT 1 FROM spaces s WHERE s.kind = 'personal' AND s.owner_user_id = u.id
);

-- 6) 题目归属空间 + 来源题
ALTER TABLE questions
    ADD COLUMN IF NOT EXISTS space_id UUID REFERENCES spaces(id),
    ADD COLUMN IF NOT EXISTS origin_question_id UUID REFERENCES questions(id) ON DELETE SET NULL;

-- 历史题目挂到创建者个人空间；无创建者则挂公共空间
UPDATE questions q
SET space_id = s.id
FROM spaces s
WHERE q.space_id IS NULL
  AND s.kind = 'personal'
  AND s.owner_user_id = q.creator_id;

UPDATE questions
SET space_id = '00000000-0000-0000-0000-000000000001'::uuid
WHERE space_id IS NULL;

ALTER TABLE questions
    ALTER COLUMN space_id SET NOT NULL;

CREATE INDEX IF NOT EXISTS idx_questions_space ON questions(space_id);
CREATE INDEX IF NOT EXISTS idx_questions_origin ON questions(origin_question_id);

-- 7) 指定审题人
CREATE TABLE IF NOT EXISTS question_reviewers (
    question_id     UUID NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    assigned_by     UUID REFERENCES users(id) ON DELETE SET NULL,
    assigned_at     TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (question_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_question_reviewers_user ON question_reviewers(user_id);

-- 8) 角色枚举收窄：admin / user（原 teacher/groupleader/viewer → user）
DO $$ BEGIN
    CREATE TYPE user_role_new AS ENUM ('admin', 'user');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

ALTER TABLE users ALTER COLUMN role DROP DEFAULT;

-- 将旧枚举映射到新枚举（若已是新类型则跳过逻辑依赖 text 比较）
ALTER TABLE users
    ALTER COLUMN role TYPE user_role_new
    USING (
        CASE
            WHEN role::text IN ('admin', 'Admin') THEN 'admin'::user_role_new
            ELSE 'user'::user_role_new
        END
    );

DROP TYPE IF EXISTS user_role;
ALTER TYPE user_role_new RENAME TO user_role;

ALTER TABLE users ALTER COLUMN role SET DEFAULT 'user'::user_role;
