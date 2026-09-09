-- MathType 公式资产：按题关联（field + ordinal），导出时按槽位嵌入 Equation.DSMT4
-- 转换链：LaTeX → MathML → ole.bin（mathtype-ole）→ WMF+baseline（mt_converter /convert_batch）

CREATE TABLE IF NOT EXISTS question_math_assets (
    id                   UUID PRIMARY KEY,
    question_id          UUID NOT NULL REFERENCES questions(id) ON DELETE CASCADE,
    -- 与录题抽公式字段一致，便于导出时按字段序号对齐
    field                TEXT NOT NULL
        CHECK (field IN ('stem', 'options', 'correct_answer', 'analysis', 'structure')),
    ordinal              INT  NOT NULL CHECK (ordinal >= 0),
    latex                TEXT NOT NULL,
    display              BOOLEAN NOT NULL DEFAULT FALSE,
    mathml               TEXT,
    ole_bin              BYTEA,
    wmf                  BYTEA,
    baseline_offset_pt   DOUBLE PRECISION,
    width_pt             DOUBLE PRECISION,
    height_pt            DOUBLE PRECISION,
    status               TEXT NOT NULL DEFAULT 'pending'
        CHECK (status IN ('pending', 'ready', 'failed', 'stale')),
    error                TEXT,
    engine               TEXT,
    created_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (question_id, field, ordinal)
);

CREATE INDEX IF NOT EXISTS idx_question_math_assets_question
    ON question_math_assets (question_id);

CREATE INDEX IF NOT EXISTS idx_question_math_assets_status
    ON question_math_assets (status)
    WHERE status IN ('pending', 'stale', 'failed');

COMMENT ON TABLE question_math_assets IS
    'Per-question MathType assets (ole.bin + placeable WMF); keyed by field+ordinal, not global hash';
