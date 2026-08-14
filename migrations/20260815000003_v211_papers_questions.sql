-- =============================================================================
-- V2.1.1 P0-B：papers / paper_questions / question_collections /
--              collection_questions / questions hash 列
--
-- 元数据归属（计划书 §三）：Paper 存试卷语义字段；QuestionCollection 存题目
-- 集合字段；Document 只存文件字段。document_type 只存在于 documents 层。
-- =============================================================================

-- ─────────────────────────────────────────────────────────────────────────────
-- 1. papers 增列（试卷元数据 + 来源 + 幂等复用键 document_id）
--    school 暂用 school_name 正式列（school 实体化列入 P2 backlog）
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE papers ADD COLUMN IF NOT EXISTS year            INT;
ALTER TABLE papers ADD COLUMN IF NOT EXISTS stage           VARCHAR(20);
ALTER TABLE papers ADD COLUMN IF NOT EXISTS semester        VARCHAR(20);
ALTER TABLE papers ADD COLUMN IF NOT EXISTS region_province VARCHAR(50);
ALTER TABLE papers ADD COLUMN IF NOT EXISTS region_city     VARCHAR(50);
ALTER TABLE papers ADD COLUMN IF NOT EXISTS school_name     VARCHAR(200);
ALTER TABLE papers ADD COLUMN IF NOT EXISTS source_type     VARCHAR(30);
ALTER TABLE papers ADD COLUMN IF NOT EXISTS sub_source_type VARCHAR(30);
ALTER TABLE papers ADD COLUMN IF NOT EXISTS document_id     UUID REFERENCES documents(id) ON DELETE SET NULL;
ALTER TABLE papers ADD COLUMN IF NOT EXISTS metadata        JSONB NOT NULL DEFAULT '{}';

CREATE INDEX IF NOT EXISTS idx_papers_document ON papers(document_id) WHERE document_id IS NOT NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- 2. paper_questions 增列：question_no（自由格式，不唯一）、display_order
--    display_order 回填 = sort_order；question_no 历史留 NULL 不猜测
--    UNIQUE(paper_id, question_id)：Worker 重试幂等 + 防重复关联
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE paper_questions ADD COLUMN IF NOT EXISTS question_no   VARCHAR(40);
ALTER TABLE paper_questions ADD COLUMN IF NOT EXISTS display_order INT;

UPDATE paper_questions SET display_order = sort_order WHERE display_order IS NULL;
ALTER TABLE paper_questions ALTER COLUMN display_order SET NOT NULL;
ALTER TABLE paper_questions ALTER COLUMN display_order SET DEFAULT 0;

-- 去重历史重复关联（保留 id 较小者）后再建唯一索引
DELETE FROM paper_questions a
USING paper_questions b
WHERE a.paper_id = b.paper_id AND a.question_id = b.question_id AND a.id < b.id;

CREATE UNIQUE INDEX IF NOT EXISTS idx_paper_questions_unique ON paper_questions(paper_id, question_id);
CREATE INDEX IF NOT EXISTS idx_paper_questions_question_no ON paper_questions(paper_id, question_no);

-- ─────────────────────────────────────────────────────────────────────────────
-- 3. question_collections（非试卷题目集合）
--    复用规则（计划书 §6.1）：同文档内按 (document_id, title) 幂等复用；
--    跨文档同名资料一律新建。
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS question_collections (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    document_id     UUID NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    creator_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    -- 集合类型：class_exercise/class_example/homework/.../other（白名单校验）
    collection_type TEXT NOT NULL,
    -- collection_type = 'other' 时的自定义名
    type_label      TEXT,
    source_type     TEXT,
    subject         TEXT,
    stage           TEXT,
    grade           TEXT,
    semester        TEXT,
    -- 章节（知识树节点，可选）
    chapter_id      UUID REFERENCES knowledge_nodes(id) ON DELETE SET NULL,
    metadata        JSONB NOT NULL DEFAULT '{}',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_collections_document_title ON question_collections(document_id, title);
CREATE INDEX IF NOT EXISTS idx_collections_creator ON question_collections(creator_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- 4. collection_questions（集合-题目关联，题号自由格式不唯一）
--    UNIQUE(collection_id, question_id)：防重试重复关联
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS collection_questions (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    collection_id UUID NOT NULL REFERENCES question_collections(id) ON DELETE CASCADE,
    question_id   UUID NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    question_no   VARCHAR(40),
    display_order INT NOT NULL DEFAULT 0,
    section       VARCHAR(100),
    score         INT,
    metadata      JSONB NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_collection_questions_unique ON collection_questions(collection_id, question_id);
CREATE INDEX IF NOT EXISTS idx_collection_questions_question ON collection_questions(question_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- 5. questions 增列：去重 hash（SHA-256，由 Rust 单点算法计算）
--    本迁移只建列；历史回填由离线 Job src/bin/backfill_question_hashes.rs 完成
--    （计划书 §八：SQL 不做第二套规范化实现）
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE questions ADD COLUMN IF NOT EXISTS content_hash            TEXT;
ALTER TABLE questions ADD COLUMN IF NOT EXISTS normalized_content_hash TEXT;

CREATE INDEX IF NOT EXISTS idx_questions_normalized_content_hash ON questions(normalized_content_hash);
