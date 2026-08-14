-- =============================================================================
-- V2.1.1 P0-C：ai_parse_tasks 扩展
--
-- Document → Task 为 1:N（document_id 不唯一）；paper_meta 为输入快照；
-- progress JSONB 存幂等映射 {question_index → question_id}；
-- locked_at/worker_id/heartbeat_at 为租约与心跳（计划书 §7.3）；
-- cancel_requested_at 为用户取消标记（计划书 §6.4）。
-- =============================================================================

ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS document_id  UUID REFERENCES documents(id) ON DELETE SET NULL;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS paper_meta   JSONB NOT NULL DEFAULT '{}';

-- 统计与进度字段（前端进度展示，计划书 §十五）
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS total_count       INT NOT NULL DEFAULT 0;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS processed_count   INT NOT NULL DEFAULT 0;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS success_count     INT NOT NULL DEFAULT 0;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS failed_count      INT NOT NULL DEFAULT 0;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS retry_count       INT NOT NULL DEFAULT 0;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS current_page      INT;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS total_pages       INT;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS current_question_no TEXT;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS started_at        TIMESTAMPTZ;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS completed_at      TIMESTAMPTZ;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS last_error        TEXT;

-- 幂等映射：{"idempotency_map": {"q_0_3": "<question_id>", ...}}
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS progress JSONB NOT NULL DEFAULT '{}';

-- 租约 / 心跳 / 取消（多 Worker 安全，计划书 §7.3）
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS locked_at           TIMESTAMPTZ;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS worker_id           TEXT;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS heartbeat_at        TIMESTAMPTZ;
ALTER TABLE ai_parse_tasks ADD COLUMN IF NOT EXISTS cancel_requested_at TIMESTAMPTZ;

CREATE INDEX IF NOT EXISTS idx_ai_tasks_document ON ai_parse_tasks(document_id);
