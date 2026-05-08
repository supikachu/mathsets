-- 题型枚举
DO $$ BEGIN
    CREATE TYPE question_type AS ENUM ('choice', 'fill', 'solution', 'judgment');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- 难度枚举
DO $$ BEGIN
    CREATE TYPE difficulty AS ENUM ('easy', 'medium', 'hard');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- 题目状态枚举
DO $$ BEGIN
    CREATE TYPE question_status AS ENUM ('draft', 'pending', 'rejected', 'published', 'disabled');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

-- 题目主表
CREATE TABLE IF NOT EXISTS questions (
    id                UUID PRIMARY KEY,
    stem              TEXT NOT NULL,
    question_type     question_type NOT NULL,
    difficulty        difficulty NOT NULL DEFAULT 'medium',
    default_score     INT NOT NULL DEFAULT 5,
    status            question_status NOT NULL DEFAULT 'draft',

    -- 题型特有数据 (JSONB)
    options           JSONB,
    correct_answer    JSONB NOT NULL DEFAULT '[]'::jsonb,
    analysis          TEXT,
    grading_criteria  JSONB,

    -- 归属信息
    grade             VARCHAR(10),
    semester          VARCHAR(10),
    source            VARCHAR(50) DEFAULT '原创',

    -- 元信息
    creator_id        UUID NOT NULL REFERENCES users(id),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_by        UUID REFERENCES users(id),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    version           INT NOT NULL DEFAULT 1
);

CREATE INDEX IF NOT EXISTS idx_questions_status ON questions(status);
CREATE INDEX IF NOT EXISTS idx_questions_type ON questions(question_type);
CREATE INDEX IF NOT EXISTS idx_questions_difficulty ON questions(difficulty);
CREATE INDEX IF NOT EXISTS idx_questions_creator ON questions(creator_id);
CREATE INDEX IF NOT EXISTS idx_questions_created_at ON questions(created_at);

-- 知识点树
CREATE TABLE IF NOT EXISTS knowledge_points (
    id                UUID PRIMARY KEY,
    parent_id         UUID REFERENCES knowledge_points(id) ON DELETE SET NULL,
    name              VARCHAR(100) NOT NULL,
    grade             VARCHAR(10),
    sort_order        INT NOT NULL DEFAULT 0,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_kp_parent ON knowledge_points(parent_id);

-- 题目 ↔ 知识点 (多对多)
CREATE TABLE IF NOT EXISTS question_knowledge_points (
    question_id       UUID NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    knowledge_point_id UUID NOT NULL REFERENCES knowledge_points(id) ON DELETE CASCADE,
    PRIMARY KEY (question_id, knowledge_point_id)
);

CREATE INDEX IF NOT EXISTS idx_qkp_question ON question_knowledge_points(question_id);
CREATE INDEX IF NOT EXISTS idx_qkp_kp ON question_knowledge_points(knowledge_point_id);

-- 版本历史（完整快照）
CREATE TABLE IF NOT EXISTS question_versions (
    id                UUID PRIMARY KEY,
    question_id       UUID NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    version           INT NOT NULL,
    snapshot          JSONB NOT NULL,
    change_summary    TEXT,
    created_by        UUID REFERENCES users(id),
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_qv_question ON question_versions(question_id);

-- 审核记录
CREATE TABLE IF NOT EXISTS review_records (
    id                UUID PRIMARY KEY,
    question_id       UUID NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    reviewer_id       UUID NOT NULL REFERENCES users(id),
    action            VARCHAR(20) NOT NULL,
    comment           TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rr_question ON review_records(question_id);
