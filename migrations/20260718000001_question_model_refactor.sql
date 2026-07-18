-- =============================================================================
-- Question 模型重构迁移
-- 新增枚举类型、内容渲染字段、复合题支持、教研维度、统计缓存
-- 恢复 creator_id NOT NULL 约束
-- =============================================================================

-- 1) 新增枚举类型

-- 年级枚举
DO $$ BEGIN
    CREATE TYPE grade_level AS ENUM (
        'grade_7', 'grade_8', 'grade_9',
        'grade_10', 'grade_11', 'grade_12',
        'other'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- 学期枚举
DO $$ BEGIN
    CREATE TYPE semester_type AS ENUM ('first', 'second', 'full_year');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- 认知层次（布鲁姆分类法）
DO $$ BEGIN
    CREATE TYPE cognitive_level AS ENUM (
        'remember', 'understand', 'apply',
        'analyze', 'evaluate', 'create'
    );
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- 2) 内容渲染增强
ALTER TABLE questions ADD COLUMN IF NOT EXISTS stem_text TEXT;
ALTER TABLE questions ADD COLUMN IF NOT EXISTS images JSONB DEFAULT '[]'::jsonb;

-- 3) 复合题 / 父子题支持
ALTER TABLE questions ADD COLUMN IF NOT EXISTS parent_id UUID REFERENCES questions(id) ON DELETE CASCADE;
ALTER TABLE questions ADD COLUMN IF NOT EXISTS sub_order SMALLINT DEFAULT 0;
CREATE INDEX IF NOT EXISTS idx_questions_parent ON questions(parent_id);

-- 4) 教研维度增强
ALTER TABLE questions ADD COLUMN IF NOT EXISTS grade_level grade_level;
ALTER TABLE questions ADD COLUMN IF NOT EXISTS semester_new semester_type;
ALTER TABLE questions ADD COLUMN IF NOT EXISTS cognitive_level cognitive_level;
ALTER TABLE questions ADD COLUMN IF NOT EXISTS difficulty_score SMALLINT CHECK (difficulty_score BETWEEN 1 AND 10);
ALTER TABLE questions ADD COLUMN IF NOT EXISTS estimated_minutes SMALLINT;

-- 5) 统计缓存字段
ALTER TABLE questions ADD COLUMN IF NOT EXISTS paper_count INT NOT NULL DEFAULT 0;
ALTER TABLE questions ADD COLUMN IF NOT EXISTS attempt_count INT NOT NULL DEFAULT 0;
ALTER TABLE questions ADD COLUMN IF NOT EXISTS accuracy_rate NUMERIC(5,4) DEFAULT NULL;
ALTER TABLE questions ADD COLUMN IF NOT EXISTS favorite_count INT NOT NULL DEFAULT 0;

-- 6) 废弃字段标注（保留不删除）
COMMENT ON COLUMN questions.grade IS 'DEPRECATED: 请使用 grade_level (grade_level ENUM) 替代';
COMMENT ON COLUMN questions.semester IS 'DEPRECATED: 请使用 semester_new (semester_type ENUM) 替代';
COMMENT ON COLUMN questions.grade_semester IS 'DEPRECATED: 请使用 grade_level + semester_new 替代';

-- 7) 恢复 creator_id NOT NULL 约束
-- 先将历史空数据填充为系统默认用户 UUID
UPDATE questions SET creator_id = '00000000-0000-0000-0000-000000000000'::uuid WHERE creator_id IS NULL;
ALTER TABLE questions ALTER COLUMN creator_id SET NOT NULL;

-- 8) 新增查询索引
CREATE INDEX IF NOT EXISTS idx_questions_grade_level ON questions(grade_level);
CREATE INDEX IF NOT EXISTS idx_questions_cognitive ON questions(cognitive_level);
CREATE INDEX IF NOT EXISTS idx_questions_stem_text_gin ON questions USING GIN (to_tsvector('simple', COALESCE(stem_text, '')));
