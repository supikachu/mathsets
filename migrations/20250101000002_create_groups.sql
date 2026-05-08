-- 教研组表
CREATE TABLE IF NOT EXISTS groups (
    id              UUID PRIMARY KEY,
    name            VARCHAR(100) NOT NULL,
    description     TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 教研组成员关联表
CREATE TABLE IF NOT EXISTS group_members (
    group_id        UUID NOT NULL REFERENCES groups(id) ON DELETE CASCADE,
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    is_leader       BOOLEAN NOT NULL DEFAULT FALSE,
    joined_at       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (group_id, user_id)
);

CREATE INDEX IF NOT EXISTS idx_group_members_user ON group_members(user_id);
CREATE INDEX IF NOT EXISTS idx_group_members_group ON group_members(group_id);
