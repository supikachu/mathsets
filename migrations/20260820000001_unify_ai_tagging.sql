-- =============================================================================
-- 统一智能打标：建议记录 + 来源追踪 + 候选目标类型
--
-- 1. ai_tagging_suggestions：编辑页 / 录题 Worker 共用的打标建议
-- 2. question_knowledge_nodes.suggestion_id：节点关联可追溯到建议
-- 3. question_tags_relation 增加 source / ai_confidence / suggestion_id
-- 4. tag_candidates 增加 target_type / suggested_tag_id，并修正幂等索引
-- 5. knowledge_nodes.name / tags.name 的 pg_trgm GIN 索引
-- =============================================================================

-- ─────────────────────────────────────────────────────────────────────────────
-- 1. 打标建议
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS ai_tagging_suggestions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_id      UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    space_id        UUID REFERENCES spaces(id) ON DELETE SET NULL,
    question_id     UUID REFERENCES questions(id) ON DELETE SET NULL,
    source_task_id  UUID REFERENCES ai_parse_tasks(id) ON DELETE SET NULL,
    source_index    TEXT,
    input_hash      TEXT NOT NULL,
    engine_version  TEXT NOT NULL,
    status          TEXT NOT NULL DEFAULT 'pending',
    result          JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    applied_at      TIMESTAMPTZ,
    CONSTRAINT chk_ai_tagging_suggestions_status
        CHECK (status IN ('pending', 'applied', 'discarded', 'expired'))
);

CREATE INDEX IF NOT EXISTS idx_ai_tagging_suggestions_creator
    ON ai_tagging_suggestions(creator_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ai_tagging_suggestions_question
    ON ai_tagging_suggestions(question_id)
    WHERE question_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_ai_tagging_suggestions_status
    ON ai_tagging_suggestions(status);

-- 录题暂存来源幂等：同一任务同一暂存 index 只保留一条建议
CREATE UNIQUE INDEX IF NOT EXISTS idx_ai_tagging_suggestions_task_index
    ON ai_tagging_suggestions (source_task_id, source_index)
    WHERE source_task_id IS NOT NULL AND source_index IS NOT NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- 2. 知识树关联追溯
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE question_knowledge_nodes
    ADD COLUMN IF NOT EXISTS suggestion_id UUID REFERENCES ai_tagging_suggestions(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_qkn_suggestion
    ON question_knowledge_nodes(suggestion_id)
    WHERE suggestion_id IS NOT NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- 3. 扁平标签关联来源（核心素养 / 通用方法）
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE question_tags_relation
    ADD COLUMN IF NOT EXISTS source knowledge_link_source NOT NULL DEFAULT 'manual';
ALTER TABLE question_tags_relation
    ADD COLUMN IF NOT EXISTS ai_confidence NUMERIC(5,4);
ALTER TABLE question_tags_relation
    ADD COLUMN IF NOT EXISTS suggestion_id UUID REFERENCES ai_tagging_suggestions(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_qtr_source ON question_tags_relation(source);
CREATE INDEX IF NOT EXISTS idx_qtr_suggestion
    ON question_tags_relation(suggestion_id)
    WHERE suggestion_id IS NOT NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- 4. 候选：目标类型 + 扁平标签指向 + 幂等索引修正
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE tag_candidates
    ADD COLUMN IF NOT EXISTS target_type TEXT NOT NULL DEFAULT 'knowledge_node';
ALTER TABLE tag_candidates
    ADD COLUMN IF NOT EXISTS suggested_tag_id UUID REFERENCES tags(id) ON DELETE SET NULL;

UPDATE tag_candidates
SET target_type = 'tag'
WHERE kind IN ('method', 'core_competence')
  AND target_type IS DISTINCT FROM 'tag';

ALTER TABLE tag_candidates DROP CONSTRAINT IF EXISTS chk_tag_candidates_target_type;
ALTER TABLE tag_candidates ADD CONSTRAINT chk_tag_candidates_target_type
    CHECK (target_type IN ('knowledge_node', 'tag'));

ALTER TABLE tag_candidates DROP CONSTRAINT IF EXISTS chk_tag_candidates_kind;
ALTER TABLE tag_candidates ADD CONSTRAINT chk_tag_candidates_kind
    CHECK (kind IN ('chapter', 'knowledge', 'pattern', 'method', 'core_competence'));

ALTER TABLE tag_candidates DROP CONSTRAINT IF EXISTS chk_tag_candidates_status;
ALTER TABLE tag_candidates ADD CONSTRAINT chk_tag_candidates_status
    CHECK (status IN ('pending', 'approved', 'rejected', 'merged'));

-- 旧唯一索引在 NULL source_task_id 上无法去重；改为部分唯一索引
DROP INDEX IF EXISTS idx_tag_candidates_dedup;

CREATE UNIQUE INDEX IF NOT EXISTS idx_tag_candidates_dedup_task
    ON tag_candidates (source_task_id, source_question_id, normalized_name, kind)
    WHERE source_task_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_tag_candidates_dedup_question
    ON tag_candidates (source_question_id, normalized_name, kind)
    WHERE source_task_id IS NULL AND source_question_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_tag_candidates_target_type ON tag_candidates(target_type);
CREATE INDEX IF NOT EXISTS idx_tag_candidates_suggested_tag
    ON tag_candidates(suggested_tag_id)
    WHERE suggested_tag_id IS NOT NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- 5. 模糊召回索引
-- ─────────────────────────────────────────────────────────────────────────────
CREATE INDEX IF NOT EXISTS idx_knowledge_nodes_name_trgm
    ON knowledge_nodes USING GIN (name gin_trgm_ops);

CREATE INDEX IF NOT EXISTS idx_tags_name_trgm
    ON tags USING GIN (name gin_trgm_ops);
