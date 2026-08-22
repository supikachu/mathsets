-- =============================================================================
-- 打标任务携带解析阶段已产出的信号
--
-- 解析阶段（Stage2）已让 LLM 产出 knowledge_points / chapter_path /
-- solution_methods，打标阶段却把题文再丢给 LLM 重抽一遍关键词（实测 19 题合计
-- 1390 秒）。入队时把已有信号一并存下，worker 判定信号足够时直接跳过 LLM 提取；
-- 信号为空或过弱仍回退到原来的 Content 提取，保证质量不退。
-- =============================================================================

ALTER TABLE ai_tagging_tasks
    ADD COLUMN IF NOT EXISTS parsed_signals JSONB;
