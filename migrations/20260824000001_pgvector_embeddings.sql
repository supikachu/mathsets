-- pgvector embeddings for tagging recall (optional).
-- If the role cannot CREATE EXTENSION or the server lacks `vector`, skip DDL
-- so sqlx migrate still succeeds (same idea as pg_trgm, but actually catch errors).

DO $$
BEGIN
  CREATE EXTENSION IF NOT EXISTS vector;
EXCEPTION
  WHEN insufficient_privilege THEN
    RAISE NOTICE 'pgvector skipped: insufficient privilege';
  WHEN undefined_file THEN
    RAISE NOTICE 'pgvector skipped: extension files not installed';
  WHEN OTHERS THEN
    RAISE NOTICE 'pgvector skipped: %', SQLERRM;
END
$$;

DO $$
BEGIN
  IF NOT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'vector') THEN
    RAISE NOTICE 'knowledge_node_embeddings skipped: vector extension unavailable';
    RETURN;
  END IF;

  CREATE TABLE IF NOT EXISTS knowledge_node_embeddings (
    node_id      UUID PRIMARY KEY REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    embedding    vector(1024) NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
  );

  CREATE TABLE IF NOT EXISTS tag_embeddings (
    tag_id       UUID PRIMARY KEY REFERENCES tags(id) ON DELETE CASCADE,
    content_hash TEXT NOT NULL,
    embedding    vector(1024) NOT NULL,
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
  );

  BEGIN
    CREATE INDEX IF NOT EXISTS idx_kn_embeddings_hnsw
      ON knowledge_node_embeddings USING hnsw (embedding vector_cosine_ops);
  EXCEPTION
    WHEN OTHERS THEN
      RAISE NOTICE 'knowledge_node_embeddings HNSW skipped: %', SQLERRM;
  END;

  BEGIN
    CREATE INDEX IF NOT EXISTS idx_tag_embeddings_hnsw
      ON tag_embeddings USING hnsw (embedding vector_cosine_ops);
  EXCEPTION
    WHEN OTHERS THEN
      RAISE NOTICE 'tag_embeddings HNSW skipped: %', SQLERRM;
  END;
END
$$;
