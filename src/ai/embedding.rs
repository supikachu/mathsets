//! DashScope text-embedding-v3 client（打标向量召回，不是 AiProvider）

use std::sync::OnceLock;
use std::time::Duration;

use dashmap::DashMap;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use uuid::Uuid;

pub const EMBEDDING_MODEL: &str = "text-embedding-v3";
pub const EMBEDDING_DIM: usize = 1024;
const BATCH_SIZE: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum EmbeddingError {
    #[error("未配置 QWEN_API_KEY")]
    NoApiKey,
    #[error("embedding HTTP: {0}")]
    Http(String),
    #[error("embedding 响应条数不匹配")]
    CountMismatch,
}

#[derive(Clone)]
pub struct EmbeddingClient {
    http: reqwest::Client,
    api_key: String,
    url: String,
}

impl EmbeddingClient {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("QWEN_API_KEY").ok().filter(|s| !s.is_empty())?;
        let base = std::env::var("QWEN_BASE_URL").unwrap_or_else(|_| {
            "https://dashscope.aliyuncs.com/compatible-mode".into()
        });
        let url = format!("{}/v1/embeddings", base.trim_end_matches('/'));
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .ok()?;
        Some(Self { http, api_key, url })
    }

    pub async fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let cache = query_cache();
        let mut out: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
        let mut missing: Vec<(usize, String)> = Vec::new();
        for (i, t) in texts.iter().enumerate() {
            if let Some(v) = cache.get(t) {
                out[i] = Some(v.clone());
            } else {
                missing.push((i, t.clone()));
            }
        }
        for chunk in missing.chunks(BATCH_SIZE) {
            let inputs: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
            let fetched = self.embed_uncached(&inputs).await?;
            if fetched.len() != inputs.len() {
                return Err(EmbeddingError::CountMismatch);
            }
            for ((idx, text), vec) in chunk.iter().zip(fetched.into_iter()) {
                cache.insert(text.clone(), vec.clone());
                out[*idx] = Some(vec);
            }
        }
        out.into_iter()
            .map(|v| v.ok_or(EmbeddingError::CountMismatch))
            .collect()
    }

    async fn embed_uncached(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>, EmbeddingError> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            model: &'a str,
            input: &'a [String],
            dimensions: usize,
        }
        #[derive(serde::Deserialize)]
        struct Data {
            embedding: Vec<f32>,
            #[serde(default)]
            index: usize,
        }
        #[derive(serde::Deserialize)]
        struct Resp {
            #[serde(default)]
            data: Vec<Data>,
            #[serde(default)]
            error: Option<serde_json::Value>,
        }

        let resp = self
            .http
            .post(&self.url)
            .bearer_auth(&self.api_key)
            .json(&Req {
                model: EMBEDDING_MODEL,
                input: inputs,
                dimensions: EMBEDDING_DIM,
            })
            .send()
            .await
            .map_err(|e| EmbeddingError::Http(e.to_string()))?;
        let status = resp.status();
        let body: Resp = resp
            .json()
            .await
            .map_err(|e| EmbeddingError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(EmbeddingError::Http(format!(
                "HTTP {status}: {}",
                body.error.unwrap_or(serde_json::Value::Null)
            )));
        }
        let mut data = body.data;
        data.sort_by_key(|d| d.index);
        Ok(data.into_iter().map(|d| d.embedding).collect())
    }
}

fn query_cache() -> &'static DashMap<String, Vec<f32>> {
    static CACHE: OnceLock<DashMap<String, Vec<f32>>> = OnceLock::new();
    CACHE.get_or_init(DashMap::new)
}

pub fn vector_recall_wanted() -> bool {
    if cfg!(test) {
        return std::env::var("TAGGING_VECTOR_RECALL_TEST").ok().as_deref() == Some("1");
    }
    std::env::var("TAGGING_VECTOR_RECALL").ok().as_deref() != Some("0")
}

pub async fn embeddings_table_ready(pool: &PgPool) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT to_regclass('public.knowledge_node_embeddings') IS NOT NULL",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false)
}

async fn vector_extension_ready(pool: &PgPool) -> bool {
    sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'vector')",
    )
    .fetch_one(pool)
    .await
    .unwrap_or(false)
}

/// 扩展后装时 sqlx 不会重跑已成功的迁移。启动时补 CREATE EXTENSION / 建表。
pub async fn ensure_embedding_schema(pool: &PgPool) -> Result<bool, String> {
    if !vector_extension_ready(pool).await {
        if let Err(e) = sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(pool)
            .await
        {
            tracing::info!("无法 CREATE EXTENSION vector: {e}");
            return Ok(false);
        }
    }
    if !vector_extension_ready(pool).await {
        return Ok(false);
    }

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS knowledge_node_embeddings (
          node_id      UUID PRIMARY KEY REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
          content_hash TEXT NOT NULL,
          embedding    vector(1024) NOT NULL,
          updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS tag_embeddings (
          tag_id       UUID PRIMARY KEY REFERENCES tags(id) ON DELETE CASCADE,
          content_hash TEXT NOT NULL,
          embedding    vector(1024) NOT NULL,
          updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    if let Err(e) = sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_kn_embeddings_hnsw
          ON knowledge_node_embeddings USING hnsw (embedding vector_cosine_ops)
        "#,
    )
    .execute(pool)
    .await
    {
        tracing::debug!("knowledge_node_embeddings HNSW 跳过: {e}");
    }
    if let Err(e) = sqlx::query(
        r#"
        CREATE INDEX IF NOT EXISTS idx_tag_embeddings_hnsw
          ON tag_embeddings USING hnsw (embedding vector_cosine_ops)
        "#,
    )
    .execute(pool)
    .await
    {
        tracing::debug!("tag_embeddings HNSW 跳过: {e}");
    }

    Ok(embeddings_table_ready(pool).await)
}

pub fn format_vector(v: &[f32]) -> String {
    let inner = v
        .iter()
        .map(|x| format!("{x:.7}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{inner}]")
}

pub fn content_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(EMBEDDING_MODEL.as_bytes());
    h.update(b":");
    h.update(EMBEDDING_DIM.to_string().as_bytes());
    h.update(b"\n");
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

pub fn aliases_from_json(v: &serde_json::Value) -> Vec<String> {
    let Some(arr) = v.as_array() else {
        return vec![];
    };
    arr.iter()
        .filter_map(|a| {
            a.get("alias")
                .and_then(|x| x.as_str())
                .or_else(|| a.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
        .collect()
}

pub fn node_embed_text(name_path: &str, name: &str, aliases: &[String]) -> String {
    format!("{}\n{}\n{}", name_path.trim(), name.trim(), aliases.join("、"))
}

pub fn tag_embed_text(category: &str, name: &str, aliases: &[String]) -> String {
    format!("{}\n{}\n{}", category.trim(), name.trim(), aliases.join("、"))
}

pub fn spawn_refresh_node_embedding(pool: PgPool, node_id: Uuid) {
    tokio::spawn(async move {
        if let Err(e) = refresh_node_embedding(&pool, node_id).await {
            tracing::debug!(node_id = %node_id, "刷新节点 embedding 跳过: {e}");
        }
    });
}

pub fn spawn_refresh_tag_embedding(pool: PgPool, tag_id: Uuid) {
    tokio::spawn(async move {
        if let Err(e) = refresh_tag_embedding(&pool, tag_id).await {
            tracing::debug!(tag_id = %tag_id, "刷新标签 embedding 跳过: {e}");
        }
    });
}

pub async fn start_backfill(pool: PgPool) {
    if !vector_recall_wanted() {
        return;
    }
    match ensure_embedding_schema(&pool).await {
        Ok(true) => {}
        Ok(false) => {
            tracing::info!(
                "向量召回未启用：PostgreSQL 未安装 pgvector（缺少 share/extension/vector.control）。仅执行 CREATE EXTENSION 不够，需把扩展文件装进当前这套服务后再重启。"
            );
            return;
        }
        Err(e) => {
            tracing::warn!("准备 embedding 表失败: {e}");
            return;
        }
    }
    let Some(client) = EmbeddingClient::from_env() else {
        tracing::info!("向量召回未启用：未配置 QWEN_API_KEY");
        return;
    };
    match backfill_all(&pool, &client).await {
        Ok((n, t)) => tracing::info!(nodes = n, tags = t, "知识树/标签 embedding 回填完成"),
        Err(e) => tracing::warn!("embedding 回填失败: {e}"),
    }
}

pub async fn refresh_node_embedding(pool: &PgPool, node_id: Uuid) -> Result<(), String> {
    if !vector_recall_wanted() || !embeddings_table_ready(pool).await {
        return Ok(());
    }
    let Some(client) = EmbeddingClient::from_env() else {
        return Ok(());
    };
    let row: Option<(String, String, serde_json::Value)> = sqlx::query_as(
        r#"
        SELECT kn.name,
               COALESCE((
                 SELECT string_agg(anc.name, ' / ' ORDER BY anc.depth)
                 FROM knowledge_nodes anc
                 WHERE anc.tree_id = kn.tree_id AND kn.path <@ anc.path AND anc.is_active = TRUE
               ), kn.name) AS name_path,
               kn.aliases
        FROM knowledge_nodes kn
        WHERE kn.id = $1
        "#,
    )
    .bind(node_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let Some((name, name_path, aliases_json)) = row else {
        return Ok(());
    };
    let aliases = aliases_from_json(&aliases_json);
    upsert_node(pool, &client, node_id, &node_embed_text(&name_path, &name, &aliases)).await
}

pub async fn refresh_tag_embedding(pool: &PgPool, tag_id: Uuid) -> Result<(), String> {
    if !vector_recall_wanted() || !embeddings_table_ready(pool).await {
        return Ok(());
    }
    let Some(client) = EmbeddingClient::from_env() else {
        return Ok(());
    };
    let row: Option<(String, String, serde_json::Value)> = sqlx::query_as(
        "SELECT name, category::text, aliases FROM tags WHERE id = $1",
    )
    .bind(tag_id)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;
    let Some((name, category, aliases_json)) = row else {
        return Ok(());
    };
    let aliases = aliases_from_json(&aliases_json);
    upsert_tag(pool, &client, tag_id, &tag_embed_text(&category, &name, &aliases)).await
}

async fn upsert_node(
    pool: &PgPool,
    client: &EmbeddingClient,
    node_id: Uuid,
    text: &str,
) -> Result<(), String> {
    let hash = content_hash(text);
    let same: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_node_embeddings WHERE node_id = $1 AND content_hash = $2)",
    )
    .bind(node_id)
    .bind(&hash)
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    if same {
        return Ok(());
    }
    let vecs = client
        .embed_texts(&[text.to_string()])
        .await
        .map_err(|e| e.to_string())?;
    let Some(v) = vecs.first() else {
        return Err("empty embedding".into());
    };
    store_node_embedding(pool, node_id, text, v).await
}

async fn upsert_tag(
    pool: &PgPool,
    client: &EmbeddingClient,
    tag_id: Uuid,
    text: &str,
) -> Result<(), String> {
    let hash = content_hash(text);
    let same: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM tag_embeddings WHERE tag_id = $1 AND content_hash = $2)",
    )
    .bind(tag_id)
    .bind(&hash)
    .fetch_one(pool)
    .await
    .unwrap_or(false);
    if same {
        return Ok(());
    }
    let vecs = client
        .embed_texts(&[text.to_string()])
        .await
        .map_err(|e| e.to_string())?;
    let Some(v) = vecs.first() else {
        return Err("empty embedding".into());
    };
    store_tag_embedding(pool, tag_id, text, v).await
}

async fn backfill_all(pool: &PgPool, client: &EmbeddingClient) -> Result<(usize, usize), String> {
    let nodes: Vec<(Uuid, String, String, serde_json::Value)> = sqlx::query_as(
        r#"
        SELECT kn.id,
               kn.name,
               COALESCE((
                 SELECT string_agg(anc.name, ' / ' ORDER BY anc.depth)
                 FROM knowledge_nodes anc
                 WHERE anc.tree_id = kn.tree_id AND kn.path <@ anc.path AND anc.is_active = TRUE
               ), kn.name) AS name_path,
               kn.aliases
        FROM knowledge_nodes kn
        WHERE kn.is_active = TRUE AND kn.status = 'active' AND kn.canonical_id IS NULL
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut pending_nodes: Vec<(Uuid, String)> = Vec::new();
    for (id, name, name_path, aliases_json) in nodes {
        let text = node_embed_text(&name_path, &name, &aliases_from_json(&aliases_json));
        let hash = content_hash(&text);
        let same: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM knowledge_node_embeddings WHERE node_id = $1 AND content_hash = $2)",
        )
        .bind(id)
        .bind(&hash)
        .fetch_one(pool)
        .await
        .unwrap_or(false);
        if !same {
            pending_nodes.push((id, text));
        }
    }
    let mut n_ok = 0usize;
    for chunk in pending_nodes.chunks(BATCH_SIZE) {
        let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
        match client.embed_texts(&texts).await {
            Ok(vecs) => {
                for ((id, text), v) in chunk.iter().zip(vecs.into_iter()) {
                    if let Err(e) = store_node_embedding(pool, *id, text, &v).await {
                        tracing::debug!(node_id = %id, "节点 embedding 回填失败: {e}");
                    } else {
                        n_ok += 1;
                    }
                }
            }
            Err(e) => tracing::warn!("节点 embedding 批次失败: {e}"),
        }
    }

    let tags: Vec<(Uuid, String, String, serde_json::Value)> = sqlx::query_as(
        r#"
        SELECT id, name, category::text, aliases
        FROM tags
        WHERE is_active = TRUE AND category::text IN ('method', 'core_competence')
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let mut pending_tags: Vec<(Uuid, String)> = Vec::new();
    for (id, name, category, aliases_json) in tags {
        let text = tag_embed_text(&category, &name, &aliases_from_json(&aliases_json));
        let hash = content_hash(&text);
        let same: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM tag_embeddings WHERE tag_id = $1 AND content_hash = $2)",
        )
        .bind(id)
        .bind(&hash)
        .fetch_one(pool)
        .await
        .unwrap_or(false);
        if !same {
            pending_tags.push((id, text));
        }
    }
    let mut t_ok = 0usize;
    for chunk in pending_tags.chunks(BATCH_SIZE) {
        let texts: Vec<String> = chunk.iter().map(|(_, t)| t.clone()).collect();
        match client.embed_texts(&texts).await {
            Ok(vecs) => {
                for ((id, text), v) in chunk.iter().zip(vecs.into_iter()) {
                    if let Err(e) = store_tag_embedding(pool, *id, text, &v).await {
                        tracing::debug!(tag_id = %id, "标签 embedding 回填失败: {e}");
                    } else {
                        t_ok += 1;
                    }
                }
            }
            Err(e) => tracing::warn!("标签 embedding 批次失败: {e}"),
        }
    }
    Ok((n_ok, t_ok))
}

async fn store_node_embedding(
    pool: &PgPool,
    node_id: Uuid,
    text: &str,
    v: &[f32],
) -> Result<(), String> {
    let hash = content_hash(text);
    let lit = format_vector(v);
    sqlx::query(
        r#"
        INSERT INTO knowledge_node_embeddings (node_id, content_hash, embedding, updated_at)
        VALUES ($1, $2, $3::vector, NOW())
        ON CONFLICT (node_id) DO UPDATE SET
          content_hash = EXCLUDED.content_hash,
          embedding = EXCLUDED.embedding,
          updated_at = NOW()
        "#,
    )
    .bind(node_id)
    .bind(&hash)
    .bind(&lit)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

async fn store_tag_embedding(
    pool: &PgPool,
    tag_id: Uuid,
    text: &str,
    v: &[f32],
) -> Result<(), String> {
    let hash = content_hash(text);
    let lit = format_vector(v);
    sqlx::query(
        r#"
        INSERT INTO tag_embeddings (tag_id, content_hash, embedding, updated_at)
        VALUES ($1, $2, $3::vector, NOW())
        ON CONFLICT (tag_id) DO UPDATE SET
          content_hash = EXCLUDED.content_hash,
          embedding = EXCLUDED.embedding,
          updated_at = NOW()
        "#,
    )
    .bind(tag_id)
    .bind(&hash)
    .bind(&lit)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_hash_changes_with_text() {
        let a = content_hash("集合\n交集\n");
        let b = content_hash("集合\n并集\n");
        assert_ne!(a, b);
        assert_eq!(a, content_hash("集合\n交集\n"));
    }

    #[test]
    fn aliases_from_json_reads_alias_field() {
        let v = serde_json::json!([{"alias": "公共元素"}, {"alias": "交"}]);
        assert_eq!(aliases_from_json(&v), vec!["公共元素", "交"]);
    }

    #[test]
    fn format_vector_uses_bracket_literal() {
        let s = format_vector(&[0.1, -0.2]);
        assert!(s.starts_with('['));
        assert!(s.ends_with(']'));
        assert!(s.contains(','));
    }
}

