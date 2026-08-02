-- =============================================================================
-- Migration: 精简 questions 数据表，清理 9 个前端完全解耦的废弃列与 4 个孤儿 ENUM
-- 日期: 2026-08-02
-- 说明:
--   1. 换算难度：基于 CEIL(difficulty_score / 2.0) 将旧难度分值换算补齐至 1-5 星级
--   2. 安全备份：使用 COALESCE(metadata, '{}'::jsonb) 规避 NULL 拼接陷阱，备份冷数据
--   3. DROP 列：删除 9 个废弃列
--   4. 清理类型：CASCADE 彻底清理 4 个孤儿 ENUM 自定义类型
-- =============================================================================

BEGIN;

-- 1. 换算补齐难度：若 difficulty 为 0 或 NULL，且 difficulty_score 有值，按 CEIL(difficulty_score / 2.0) 换算为 1-5 星级
UPDATE questions
SET difficulty = GREATEST(1, LEAST(5, CEIL(difficulty_score / 2.0)::smallint))
WHERE (difficulty IS NULL OR difficulty = 0)
  AND difficulty_score IS NOT NULL;

-- 2. 安全合并冷数据到 metadata：使用 COALESCE 规避 NULL 陷阱，使用 jsonb_strip_nulls 过滤空键
UPDATE questions
SET metadata = COALESCE(metadata, '{}'::jsonb)
    || jsonb_strip_nulls(jsonb_build_object(
        'legacy_source',             COALESCE(NULLIF(source, ''), NULL),
        'legacy_default_score',      default_score,
        'legacy_grading_criteria',   grading_criteria,
        'legacy_estimated_minutes',  estimated_minutes,
        'legacy_grade_level',        grade_level::text,
        'legacy_semester',           semester::text,
        'legacy_cognitive_level',    cognitive_level::text,
        'legacy_exam_type',          exam_type::text
    ))
WHERE source IS NOT NULL 
   OR grading_criteria IS NOT NULL 
   OR estimated_minutes IS NOT NULL
   OR grade_level IS NOT NULL
   OR semester IS NOT NULL
   OR cognitive_level IS NOT NULL
   OR exam_type IS NOT NULL;

-- 3. 删除 9 个废弃列
ALTER TABLE questions DROP COLUMN IF EXISTS default_score;
ALTER TABLE questions DROP COLUMN IF EXISTS grading_criteria;
ALTER TABLE questions DROP COLUMN IF EXISTS difficulty_score;
ALTER TABLE questions DROP COLUMN IF EXISTS estimated_minutes;
ALTER TABLE questions DROP COLUMN IF EXISTS cognitive_level;
ALTER TABLE questions DROP COLUMN IF EXISTS grade_level;
ALTER TABLE questions DROP COLUMN IF EXISTS semester;
ALTER TABLE questions DROP COLUMN IF EXISTS source;
ALTER TABLE questions DROP COLUMN IF EXISTS exam_type;

-- 4. 彻底清理 4 个孤儿 ENUM 类型
DROP TYPE IF EXISTS grade_level CASCADE;
DROP TYPE IF EXISTS semester_type CASCADE;
DROP TYPE IF EXISTS cognitive_level CASCADE;
DROP TYPE IF EXISTS exam_type CASCADE;

COMMIT;
