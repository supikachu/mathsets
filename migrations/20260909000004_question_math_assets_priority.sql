-- MathType 资产队列优先级：Worker ORDER BY priority DESC（导出/提交可提权）
ALTER TABLE question_math_assets
    ADD COLUMN IF NOT EXISTS priority INT NOT NULL DEFAULT 0;

CREATE INDEX IF NOT EXISTS idx_question_math_assets_queue
    ON question_math_assets (priority DESC, updated_at ASC)
    WHERE status IN ('pending', 'stale');

COMMENT ON COLUMN question_math_assets.priority IS
    'Conversion queue priority: 0=AI defer, 10=edit/submit, 100=export Mathtype wait';
