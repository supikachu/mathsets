-- M4：扩展 ai_parse_tasks 支持 image/pdf 异步解析任务
--
-- 新增字段：
--   source_type             — 任务来源类型（text / image / pdf），默认 text 保持兼容
--   image_b64               — base64 编码的图片数据（source_type=image 时填）
--   pdf_bytes               — 原始 PDF 二进制（source_type=pdf 时填）
--   ocr_provider_override   — 可选 OCR 引擎覆盖（用户上传时临时指定）
--   question_ids            — 多题批处理时所有生成的 question UUID 数组（JSONB）
--
-- 设计要点：
--   1. raw_text 由 NOT NULL 改为 NULL，image/pdf 任务不填 raw_text
--   2. question_id 保留为单个 UUID，存首题 ID（向后兼容旧前端）
--   3. question_ids 为 JSONB 数组，前端轮询时优先读取此字段以支持多题批处理
--   4. 大字段（image_b64/pdf_bytes）在任务完成后由 worker 主动清空，释放空间

DO $$ BEGIN
    CREATE TYPE ai_task_source_type AS ENUM ('text', 'image', 'pdf');
EXCEPTION WHEN duplicate_object THEN NULL;
END $$;

ALTER TABLE ai_parse_tasks
    ADD COLUMN IF NOT EXISTS source_type           ai_task_source_type NOT NULL DEFAULT 'text',
    ADD COLUMN IF NOT EXISTS image_b64            TEXT,
    ADD COLUMN IF NOT EXISTS pdf_bytes             BYTEA,
    ADD COLUMN IF NOT EXISTS ocr_provider_override TEXT,
    ADD COLUMN IF NOT EXISTS question_ids          JSONB;

-- 旧任务表 raw_text 强制 NOT NULL，扩展后允许 NULL（image/pdf 任务无文本）
ALTER TABLE ai_parse_tasks ALTER COLUMN raw_text DROP NOT NULL;

-- 评论：worker 完成后清空媒体字段，避免长期占用 DB 空间
COMMENT ON COLUMN ai_parse_tasks.image_b64 IS 'M4: base64 图片数据，任务完成后由 worker 清空';
COMMENT ON COLUMN ai_parse_tasks.pdf_bytes IS 'M4: PDF 原始二进制，任务完成后由 worker 清空';
COMMENT ON COLUMN ai_parse_tasks.question_ids IS 'M4: 多题批处理的所有 question UUID 数组（JSONB）';
