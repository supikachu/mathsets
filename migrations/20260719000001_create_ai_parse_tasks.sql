-- =============================================================================
-- AI 智能解析异步任务队列 (AI Parse Tasks)
-- 处理大模型（LLM）耗时解析请求的后台任务表
-- =============================================================================

-- ┌───────────────────────────────────────────────────────────────────────────┐
-- │ 1) 任务状态枚举 (ai_task_status)                                          │
-- │    pending    = 排队中，等待 worker 拾取                                  │
-- │    processing = 解析中，LLM 正在处理                                      │
-- │    completed  = 成功，已生成题目（question_id 已填入）                     │
-- │    failed     = 失败，error_message 记录详细原因                          │
-- └───────────────────────────────────────────────────────────────────────────┘

DO $$ BEGIN
    CREATE TYPE ai_task_status AS ENUM ('pending', 'processing', 'completed', 'failed');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- ┌───────────────────────────────────────────────────────────────────────────┐
-- │ 2) 任务表 (ai_parse_tasks)                                                │
-- └───────────────────────────────────────────────────────────────────────────┘

CREATE TABLE IF NOT EXISTS ai_parse_tasks (
    id            UUID           PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_id    UUID           NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    raw_text      TEXT           NOT NULL,
    status        ai_task_status NOT NULL DEFAULT 'pending',
    question_id   UUID           REFERENCES questions(id) ON DELETE SET NULL,
    error_message TEXT,
    created_at    TIMESTAMPTZ    NOT NULL DEFAULT NOW(),
    updated_at    TIMESTAMPTZ    NOT NULL DEFAULT NOW()
);

-- ┌───────────────────────────────────────────────────────────────────────────┐
-- │ 3) 索引：加速常见查询                                                     │
-- │    - idx_ai_tasks_creator:        查询某教师的历史任务列表                │
-- │    - idx_ai_tasks_status:         Worker 轮询待处理任务                  │
-- │    - idx_ai_tasks_status_created: 队列按时间排序（FIFO 出队）            │
-- └───────────────────────────────────────────────────────────────────────────┘

CREATE INDEX IF NOT EXISTS idx_ai_tasks_creator        ON ai_parse_tasks(creator_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ai_tasks_status         ON ai_parse_tasks(status);
CREATE INDEX IF NOT EXISTS idx_ai_tasks_status_created ON ai_parse_tasks(status, created_at);
