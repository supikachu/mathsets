-- 自定义 OpenAI 兼容 LLM（OpenRouter / 中转站）：用户可填 Base URL
-- 同时放宽模型名长度，OpenRouter 完整 ID 常超过 50 字符

ALTER TABLE user_ai_settings
    ADD COLUMN IF NOT EXISTS llm_base_url TEXT;

ALTER TABLE user_ai_settings
    ALTER COLUMN provider TYPE VARCHAR(32),
    ALTER COLUMN model_text TYPE TEXT,
    ALTER COLUMN model_vision TYPE TEXT;
