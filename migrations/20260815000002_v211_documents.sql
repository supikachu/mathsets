-- =============================================================================
-- V2.1.1 P0-A：documents 表（资料/Document 层）
--
-- 方案 A（TD-3）：documents.id 即文件实体 ID，不新建 files 表、不设 file_id 列。
-- 页面图片落盘 upload_dir/documents/{id}/page_{n}.{ext}（应用层管理，不入库）。
-- document_type 为 TEXT + 后端白名单校验（TD-2），confirmed 前为 NULL。
-- =============================================================================

CREATE TABLE IF NOT EXISTS documents (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    creator_id        UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    -- 原始文件信息（属于 Document 层的元数据，见计划书 §三）
    file_name         TEXT NOT NULL,
    file_size         BIGINT,
    mime              VARCHAR(100),
    page_count        INT NOT NULL DEFAULT 1,
    -- 业务分类（AI 推荐 → 用户确认后落库）
    document_type     TEXT,
    -- document_type = 'other' 时的自定义类型名（如"校本资料/竞赛资料"）
    type_label        TEXT,
    title             TEXT,
    source_type       TEXT,
    sub_source_type   TEXT,
    -- 生命周期：uploaded/classifying/classified/confirmed/parsing/done/failed/cancelled
    status            TEXT NOT NULL DEFAULT 'uploaded',
    -- AI 分类结果：{document_type,title,confidence,reason,level,checked_pages}
    ai_classification JSONB,
    -- 扩展信息：confirm 后保存 paper_meta 快照与 collections 快照
    metadata          JSONB NOT NULL DEFAULT '{}',
    -- TD-1：PDF 转换引擎标识（pdfjs / doc2x / mineru），后续 adapter 接入时写入
    conversion_engine TEXT,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_documents_creator ON documents(creator_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_documents_status  ON documents(status);
