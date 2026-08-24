-- =============================================================================
-- V2.1.1 P1：标签治理（计划书 §五/§六.3）
--
-- 1. knowledge_nodes 增列：canonical_id / status / source
--    - canonical_id：合并目标（不物理删除；CHECK 禁止自指）
--    - status：pending_review / active / merged / deprecated / rejected（生命周期）
--    - source：system / admin / ai / import（谁创建的）
--    - is_active 保持"软删除"语义，与 status 职责分离（评审意见⑧/⑯）
-- 2. tag_candidates：AI 未匹配标签 → 候选审核队列（不阻塞题目落库）
-- 3. tag_merge_records：合并审计（不物理删除）
-- =============================================================================

-- ─────────────────────────────────────────────────────────────────────────────
-- 1. knowledge_nodes 增列
-- ─────────────────────────────────────────────────────────────────────────────
ALTER TABLE knowledge_nodes ADD COLUMN IF NOT EXISTS canonical_id UUID REFERENCES knowledge_nodes(id) ON DELETE SET NULL;
ALTER TABLE knowledge_nodes ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'active';
ALTER TABLE knowledge_nodes ADD COLUMN IF NOT EXISTS source  TEXT NOT NULL DEFAULT 'system';

-- 自指禁止
ALTER TABLE knowledge_nodes DROP CONSTRAINT IF EXISTS chk_kn_canonical_not_self;
ALTER TABLE knowledge_nodes ADD CONSTRAINT chk_kn_canonical_not_self
    CHECK (canonical_id IS NULL OR canonical_id <> id);

-- 生命周期白名单
ALTER TABLE knowledge_nodes DROP CONSTRAINT IF EXISTS chk_kn_status;
ALTER TABLE knowledge_nodes ADD CONSTRAINT chk_kn_status
    CHECK (status IN ('pending_review', 'active', 'merged', 'deprecated', 'rejected'));

CREATE INDEX IF NOT EXISTS idx_kn_canonical ON knowledge_nodes(canonical_id) WHERE canonical_id IS NOT NULL;
CREATE INDEX IF NOT EXISTS idx_kn_status    ON knowledge_nodes(status);

-- 历史数据回填（计划书 §八）：status=active、source=system
UPDATE knowledge_nodes SET status = 'active', source = 'system' WHERE status IS NULL OR source IS NULL;

-- ─────────────────────────────────────────────────────────────────────────────
-- 2. tag_candidates（候选审核队列）
--    幂等键：同一任务同一题目同一规范化标签只产生一条候选
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS tag_candidates (
    id                 UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    -- 维度：chapter / knowledge / method
    kind               TEXT NOT NULL,
    raw_name           TEXT NOT NULL,
    normalized_name    TEXT NOT NULL,
    suggested_node_id  UUID REFERENCES knowledge_nodes(id) ON DELETE SET NULL,
    ai_confidence      NUMERIC(5,4),
    match_score        NUMERIC(5,4),
    source_task_id     UUID REFERENCES ai_parse_tasks(id) ON DELETE SET NULL,
    source_question_id UUID REFERENCES questions(id) ON DELETE CASCADE,
    -- pending / approved / rejected / merged
    status             TEXT NOT NULL DEFAULT 'pending',
    reviewed_by        UUID REFERENCES users(id) ON DELETE SET NULL,
    reviewed_at        TIMESTAMPTZ,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_tag_candidates_dedup
    ON tag_candidates(source_task_id, source_question_id, normalized_name, kind);
CREATE INDEX IF NOT EXISTS idx_tag_candidates_status ON tag_candidates(status);
CREATE INDEX IF NOT EXISTS idx_tag_candidates_question ON tag_candidates(source_question_id);

-- ─────────────────────────────────────────────────────────────────────────────
-- 3. tag_merge_records（合并审计）
--    target_type：knowledge_node（当前）/ tag / candidate（未来扩展）
-- ─────────────────────────────────────────────────────────────────────────────
CREATE TABLE IF NOT EXISTS tag_merge_records (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    target_type   TEXT NOT NULL DEFAULT 'knowledge_node',
    source_tag_id UUID NOT NULL,
    target_tag_id UUID NOT NULL,
    operator_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    operator_type TEXT NOT NULL DEFAULT 'admin',
    reason        TEXT,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_tag_merge_records_source ON tag_merge_records(source_tag_id);
CREATE INDEX IF NOT EXISTS idx_tag_merge_records_target ON tag_merge_records(target_tag_id);
