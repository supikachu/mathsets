-- =============================================================================
-- 解析任务入队打标：回写 staged_questions 需要来源任务 + 题内 index
-- =============================================================================

ALTER TABLE ai_tagging_tasks
    ADD COLUMN IF NOT EXISTS parse_task_id UUID REFERENCES ai_parse_tasks(id) ON DELETE SET NULL;

ALTER TABLE ai_tagging_tasks
    ADD COLUMN IF NOT EXISTS source_index TEXT;

CREATE INDEX IF NOT EXISTS idx_ai_tagging_tasks_parse_task
    ON ai_tagging_tasks(parse_task_id)
    WHERE parse_task_id IS NOT NULL;
