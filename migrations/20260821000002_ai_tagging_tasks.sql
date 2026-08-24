-- =============================================================================
-- 独立异步打标任务（不复用 ai_parse_tasks）
--
-- 题文存在任务行上，供 Worker 读取；ai_tagging_suggestions 仍不保存完整题干。
-- =============================================================================

CREATE TABLE IF NOT EXISTS ai_tagging_tasks (
    id                    UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_id            UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    space_id              UUID REFERENCES spaces(id) ON DELETE SET NULL,
    question_id           UUID REFERENCES questions(id) ON DELETE SET NULL,
    input_hash            TEXT NOT NULL,
    content               TEXT NOT NULL,
    status                TEXT NOT NULL DEFAULT 'pending',
    retry_count           INTEGER NOT NULL DEFAULT 0,
    error_message         TEXT,
    suggestion_id         UUID REFERENCES ai_tagging_suggestions(id) ON DELETE SET NULL,
    locked_at             TIMESTAMPTZ,
    worker_id             TEXT,
    heartbeat_at          TIMESTAMPTZ,
    started_at            TIMESTAMPTZ,
    completed_at          TIMESTAMPTZ,
    cancel_requested_at   TIMESTAMPTZ,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    CONSTRAINT chk_ai_tagging_tasks_status
        CHECK (status IN ('pending', 'processing', 'retrying', 'success', 'failed', 'cancelled'))
);

CREATE INDEX IF NOT EXISTS idx_ai_tagging_tasks_creator
    ON ai_tagging_tasks(creator_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_ai_tagging_tasks_status
    ON ai_tagging_tasks(status, created_at ASC)
    WHERE status IN ('pending', 'processing', 'retrying');

-- 同一用户 + 空间 + 输入哈希，进行中任务只保留一条（幂等复用）
CREATE UNIQUE INDEX IF NOT EXISTS idx_ai_tagging_tasks_inflight
    ON ai_tagging_tasks (
        creator_id,
        (COALESCE(space_id, '00000000-0000-0000-0000-000000000000'::uuid)),
        input_hash
    )
    WHERE status IN ('pending', 'processing', 'retrying');
