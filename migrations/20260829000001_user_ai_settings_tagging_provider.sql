-- =============================================================================
-- 打标独立服务商 + 解析/打标并发
--
-- 解析（Stage2）与打标此前共用 user_ai_settings.provider / api_key / llm_base_url，
-- 无法做到「OpenRouter 跑 stealth/ox-alpha + DeepSeek 官方跑 deepseek-chat」。
-- tagging_* 为空时仍回退文本槽位，既有部署行为不变。
--
-- 并发：stage2_concurrency / tagging_concurrency 为空时用环境变量/启发式默认。
-- =============================================================================

ALTER TABLE user_ai_settings
    ADD COLUMN IF NOT EXISTS tagging_provider VARCHAR(32),
    ADD COLUMN IF NOT EXISTS tagging_api_key_enc BYTEA,
    ADD COLUMN IF NOT EXISTS tagging_api_key_iv BYTEA,
    ADD COLUMN IF NOT EXISTS tagging_llm_base_url TEXT,
    ADD COLUMN IF NOT EXISTS stage2_concurrency SMALLINT,
    ADD COLUMN IF NOT EXISTS tagging_concurrency SMALLINT;
