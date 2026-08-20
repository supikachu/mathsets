-- 异步打标任务携带学段，召回时只匹配对应初中/高中知识树
ALTER TABLE ai_tagging_tasks
  ADD COLUMN IF NOT EXISTS stage VARCHAR(16);

COMMENT ON COLUMN ai_tagging_tasks.stage IS 'junior | senior（高中对应树 code 后缀 _high）';
