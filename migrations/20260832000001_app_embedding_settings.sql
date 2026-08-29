-- 全站 embedding 模型（管理员可切换）。库表固定 vector(1024)，不可混用不同维数。

CREATE TABLE IF NOT EXISTS app_embedding_settings (
  id         SMALLINT PRIMARY KEY CHECK (id = 1),
  model      TEXT NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_by UUID REFERENCES users(id) ON DELETE SET NULL
);

INSERT INTO app_embedding_settings (id, model)
VALUES (1, 'text-embedding-v3')
ON CONFLICT (id) DO NOTHING;
