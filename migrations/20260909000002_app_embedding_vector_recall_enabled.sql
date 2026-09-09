-- 全站向量召回开关（管理员可在设置页切换）。运维仍可用 TAGGING_VECTOR_RECALL=0 硬关。

ALTER TABLE app_embedding_settings
  ADD COLUMN IF NOT EXISTS enabled BOOLEAN NOT NULL DEFAULT true;
