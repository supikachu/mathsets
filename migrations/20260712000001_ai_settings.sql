-- 用户 AI 配置表
-- 存储用户个人的 LLM API Key（AES-256-GCM 加密）与服务商偏好
CREATE TABLE IF NOT EXISTS user_ai_settings (
    user_id         UUID PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    provider        VARCHAR(20) NOT NULL DEFAULT 'deepseek',  -- deepseek | qwen | openai
    api_key_enc     BYTEA,                 -- AES-256-GCM 加密后的用户 API Key
    api_key_iv      BYTEA,                 -- GCM nonce（12 字节）
    model_text      VARCHAR(50),           -- 可选覆盖，如 'deepseek-chat'
    model_vision    VARCHAR(50),           -- 可选覆盖，如 'qwen-vl-plus'
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
