-- 为知识点树添加 space_id 列，支持按空间隔离
-- 已有数据（visualtest 创建的知识点）保持 space_id = NULL，作为全局默认知识点

ALTER TABLE knowledge_points ADD COLUMN IF NOT EXISTS space_id UUID REFERENCES spaces(id) ON DELETE CASCADE;

CREATE INDEX IF NOT EXISTS idx_kp_space ON knowledge_points(space_id);
