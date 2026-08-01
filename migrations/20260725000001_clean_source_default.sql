-- =============================================================================
-- 清理 questions.source 列的"原创"幽灵默认值
-- 1. 将 source = '原创' 的历史数据置为 NULL（纯文本无实际语义）
-- 2. 移除列的 DEFAULT 约束，防止新插入数据再次产生"原创"
-- =============================================================================

UPDATE questions SET source = NULL WHERE source = '原创';

ALTER TABLE questions ALTER COLUMN source DROP DEFAULT;

COMMENT ON COLUMN questions.source IS '纯文本来源标记（如学校名、教材名），无默认值。试卷关联请使用 paper_questions 表';
