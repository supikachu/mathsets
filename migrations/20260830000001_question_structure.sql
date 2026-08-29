-- =============================================================================
-- 解答题题内嵌套结构（structure JSONB）
-- 选择题 / 填空题保持 structure IS NULL；解答题以 structure 为唯一结构来源。
-- =============================================================================

ALTER TABLE questions
    ADD COLUMN IF NOT EXISTS structure JSONB;

ALTER TABLE questions
    DROP CONSTRAINT IF EXISTS questions_structure_type_chk;

ALTER TABLE questions
    ADD CONSTRAINT questions_structure_type_chk
    CHECK (structure IS NULL OR question_type = 'solution');

COMMENT ON COLUMN questions.structure IS
    '解答题问树 {version, parts[]}；分支仅 stem，叶子含 answer/analyses；其它题型必须为 NULL';
