-- 全站 embedding 凭证：管理员可在设置页配置；未填时回退 QWEN_API_KEY / QWEN_BASE_URL。

ALTER TABLE app_embedding_settings
  ADD COLUMN IF NOT EXISTS api_key_enc BYTEA,
  ADD COLUMN IF NOT EXISTS api_key_iv BYTEA,
  ADD COLUMN IF NOT EXISTS base_url TEXT;
