-- M3：扩展 user_ai_settings 支持 OCR 引擎配置
--
-- 新增字段：
--   ocr_provider          — OCR 引擎选择（auto / doc2x / mineru / qwen_vl），默认 auto
--   doc2x_api_key_enc     — Doc2X 用户个人 API Key（AES-256-GCM 密文）
--   doc2x_api_key_iv      — 上述密文的 GCM nonce（12 字节）
--   mineru_api_endpoint   — MinerU 私有部署端点（明文，用户自填）
--   mineru_api_key_enc    — MinerU API Key（AES-256-GCM 密文，M4 启用）
--   mineru_api_key_iv     — 上述密文的 GCM nonce（12 字节，M4 启用）
--
-- 设计：与现有 api_key_enc / api_key_iv 保持一致的「密文 + nonce」双列模式，
--       每次更新 Key 时重新生成 nonce。

ALTER TABLE user_ai_settings
    ADD COLUMN IF NOT EXISTS ocr_provider        TEXT        NOT NULL DEFAULT 'auto',
    ADD COLUMN IF NOT EXISTS doc2x_api_key_enc   BYTEA,
    ADD COLUMN IF NOT EXISTS doc2x_api_key_iv    BYTEA,
    ADD COLUMN IF NOT EXISTS mineru_api_endpoint TEXT,
    ADD COLUMN IF NOT EXISTS mineru_api_key_enc  BYTEA,
    ADD COLUMN IF NOT EXISTS mineru_api_key_iv   BYTEA;
