-- AI 用量日志表（资损熔断用 — 记录每次 OCR 调用，按用户+日期统计额度）
CREATE TABLE IF NOT EXISTS ai_usage_log (
    id          BIGSERIAL PRIMARY KEY,
    user_id     UUID NOT NULL REFERENCES users(id),
    endpoint    VARCHAR(64) NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- 按用户+日期查询的索引（额度校验高频查询）
CREATE INDEX IF NOT EXISTS idx_ai_usage_user_date ON ai_usage_log (user_id, created_at);
