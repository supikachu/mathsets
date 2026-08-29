//! 向量召回：字面结果 ∪ ANN，命中一律 fuzzy 且分数封顶 0.80

use std::collections::HashMap;
use std::time::Instant;

use sqlx::PgPool;
use uuid::Uuid;

use crate::ai::embedding::{
    embeddings_table_ready, format_vector, vector_recall_wanted, EmbeddingClient,
};

use super::repository::NodeCandidate;
use super::types::{TaggingDimension, TaggingMatchType, TaggingPolicy};

pub const VECTOR_SCORE_CAP: f32 = 0.80;
pub const VECTOR_TOP_K: i64 = 8;

pub fn cap_vector_score(cosine_similarity: f32) -> f32 {
    cosine_similarity.clamp(0.0, VECTOR_SCORE_CAP)
}

fn merge_source_keys(dst: &mut Vec<String>, src: &[String]) {
    for k in src {
        if !dst.iter().any(|x| x == k) {
            dst.push(k.clone());
        }
    }
}

/// 按 `node_id` 合并。exact/alias 的类型与分数不被 fuzzy/向量覆盖。
pub fn merge_node_candidate(merged: &mut HashMap<Uuid, NodeCandidate>, c: NodeCandidate) {
    match merged.get_mut(&c.id) {
        Some(existing) => {
            merge_source_keys(&mut existing.source_keys, &c.source_keys);
            merge_source_keys(&mut existing.deterministic_keys, &c.deterministic_keys);
            let incoming_det = c.match_type.is_deterministic();
            let existing_det = existing.match_type.is_deterministic();
            if incoming_det && !existing_det {
                existing.score = c.score;
                existing.match_type = c.match_type;
                existing.name_path = c.name_path.clone();
                existing.name = c.name.clone();
                existing.path = c.path.clone();
                existing.depth = c.depth;
            } else if existing_det && !incoming_det {
                // 保留确定性类型与其分数
            } else if incoming_det && existing_det {
                if c.score > existing.score {
                    existing.score = c.score;
                    existing.match_type = c.match_type;
                    existing.name_path = c.name_path.clone();
                }
            } else if c.score > existing.score {
                existing.score = c.score;
                existing.name_path = c.name_path.clone();
            }
        }
        None => {
            merged.insert(c.id, c);
        }
    }
}

pub struct VectorRecallStats {
    pub hits: usize,
    pub elapsed_ms: u64,
}

pub async fn vector_ready(pool: &PgPool) -> bool {
    vector_recall_wanted() && embeddings_table_ready(pool).await
}

/// 为每个查询词做向量 top-k，返回待并入的 fuzzy 候选。
pub async fn recall_nodes_vector(
    pool: &PgPool,
    keys: &[String],
    dim: TaggingDimension,
    policy: &TaggingPolicy,
    space_id: Option<Uuid>,
    stage: Option<&str>,
) -> Result<(Vec<NodeCandidate>, VectorRecallStats), String> {
    let started = Instant::now();
    if !vector_ready(pool).await {
        return Ok((
            vec![],
            VectorRecallStats {
                hits: 0,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
        ));
    }
    let Some(tree_kind) = dim.tree_kind() else {
        return Ok((
            vec![],
            VectorRecallStats {
                hits: 0,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
        ));
    };
    let Some(client) = EmbeddingClient::from_pool(pool).await else {
        return Ok((
            vec![],
            VectorRecallStats {
                hits: 0,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
        ));
    };

    let mut unique_keys: Vec<String> = Vec::new();
    for k in keys {
        let k = k.trim();
        if k.is_empty() {
            continue;
        }
        if !unique_keys.iter().any(|x| x == k) {
            unique_keys.push(k.to_string());
        }
    }
    if unique_keys.is_empty() {
        return Ok((
            vec![],
            VectorRecallStats {
                hits: 0,
                elapsed_ms: started.elapsed().as_millis() as u64,
            },
        ));
    }

    let embeddings = client
        .embed_texts(&unique_keys)
        .await
        .map_err(|e| e.to_string())?;

    let leaf_only = dim.leaf_only();
    let stage_suffix = super::repository::tree_stage_code_suffix(stage);
    let code_infix: Option<&str> = if stage_suffix.is_some() {
        match dim {
            TaggingDimension::Chapter => Some("chapter"),
            TaggingDimension::Knowledge => Some("knowledge"),
            TaggingDimension::Pattern => Some("method"),
            _ => None,
        }
    } else {
        None
    };

    let mut out = Vec::new();
    for (key, vec) in unique_keys.iter().zip(embeddings.into_iter()) {
        let hits = search_nodes_by_embedding(
            pool,
            &vec,
            key,
            tree_kind,
            leaf_only,
            space_id,
            stage_suffix,
            code_infix,
            policy.fuzzy_threshold,
        )
        .await?;
        out.extend(hits);
    }

    let hits = out.len();
    Ok((
        out,
        VectorRecallStats {
            hits,
            elapsed_ms: started.elapsed().as_millis() as u64,
        },
    ))
}

/// 字面未命中时，用向量 Top1 补一条 fuzzy 标签。
pub async fn vector_match_tag_top1(
    pool: &PgPool,
    name: &str,
    category: &str,
    space_id: Option<Uuid>,
    min_score: f32,
) -> Result<Option<(Uuid, String, String, f32)>, String> {
    if !vector_ready(pool).await {
        return Ok(None);
    }
    let Some(client) = EmbeddingClient::from_pool(pool).await else {
        return Ok(None);
    };
    let key = name.trim();
    if key.is_empty() {
        return Ok(None);
    }
    let embeddings = client
        .embed_texts(&[key.to_string()])
        .await
        .map_err(|e| e.to_string())?;
    let Some(vec) = embeddings.into_iter().next() else {
        return Ok(None);
    };
    if vec.len() != crate::ai::embedding::EMBEDDING_DIM {
        return Ok(None);
    }
    let lit = format_vector(&vec);
    let row: Option<(Uuid, String, String, f32)> = sqlx::query_as(
        r#"
        SELECT
          t.id,
          t.name,
          t.category::text,
          LEAST($5::real, GREATEST(0.0::real, (1.0 - (e.embedding <=> $1::vector))::real)) AS score
        FROM tag_embeddings e
        JOIN tags t ON t.id = e.tag_id
        WHERE t.is_active = TRUE
          AND t.category::text = $2
          AND (t.space_id IS NULL OR t.space_id = $3)
        ORDER BY e.embedding <=> $1::vector
        LIMIT 1
        "#,
    )
    .bind(&lit)
    .bind(category)
    .bind(space_id)
    .bind(min_score)
    .bind(VECTOR_SCORE_CAP)
    .fetch_optional(pool)
    .await
    .map_err(|e| e.to_string())?;

    match row {
        Some((id, tag_name, cat, score)) if score >= min_score => {
            Ok(Some((id, tag_name, cat, score)))
        }
        _ => Ok(None),
    }
}

/// 用已有查询向量做节点 ANN（测试可注入假向量，生产由 `recall_nodes_vector` 调用）。
pub async fn search_nodes_by_embedding(
    pool: &PgPool,
    query_vec: &[f32],
    query_key: &str,
    tree_kind: &str,
    leaf_only: bool,
    space_id: Option<Uuid>,
    stage_suffix: Option<&str>,
    code_infix: Option<&str>,
    min_score: f32,
) -> Result<Vec<NodeCandidate>, String> {
    if query_vec.len() != crate::ai::embedding::EMBEDDING_DIM {
        return Err(format!("embedding 维数不是 1024: {}", query_vec.len()));
    }
    let lit = format_vector(query_vec);
    let rows: Vec<(Uuid, String, Uuid, String, i16, f32, String)> = sqlx::query_as(
        r#"
        SELECT
          kn.id,
          kn.name,
          kn.tree_id,
          kn.path::text,
          kn.depth,
          LEAST($8::real, GREATEST(0.0::real, (1.0 - (emb.embedding <=> $1::vector))::real)) AS score,
          COALESCE((
            SELECT string_agg(anc.name, ' / ' ORDER BY anc.depth)
            FROM knowledge_nodes anc
            WHERE anc.tree_id = kn.tree_id
              AND kn.path <@ anc.path
              AND anc.is_active = TRUE
          ), kn.name) AS name_path
        FROM knowledge_node_embeddings emb
        JOIN knowledge_nodes kn ON kn.id = emb.node_id
        JOIN knowledge_trees kt ON kt.id = kn.tree_id
        WHERE kn.is_active = TRUE
          AND kn.status = 'active'
          AND kn.canonical_id IS NULL
          AND kt.is_active = TRUE
          AND (kt.space_id IS NULL OR kt.space_id = $2)
          AND (
            ($3 = 'ability' AND (
              kt.kind::text = 'ability'
              OR (kt.kind::text = 'knowledge' AND kt.code LIKE '%_method_%')
            ))
            OR ($3 <> 'ability' AND kt.kind::text = $3)
          )
          AND ($6::text IS NULL OR kt.code LIKE '%_' || $6)
          AND ($7::text IS NULL OR kt.code LIKE '%_' || $7 || '_%')
          AND (
            NOT $4 OR NOT EXISTS (
              SELECT 1 FROM knowledge_nodes child
              WHERE child.parent_id = kn.id AND child.is_active = TRUE
            )
          )
        ORDER BY emb.embedding <=> $1::vector
        LIMIT $5
        "#,
    )
    .bind(&lit)
    .bind(space_id)
    .bind(tree_kind)
    .bind(leaf_only)
    .bind(VECTOR_TOP_K)
    .bind(stage_suffix)
    .bind(code_infix)
    .bind(VECTOR_SCORE_CAP)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(rows
        .into_iter()
        .filter(|r| r.5 >= min_score)
        .map(|(id, name, tree_id, path, depth, score, name_path)| NodeCandidate {
            id,
            name,
            tree_id,
            path,
            depth,
            score,
            match_type: TaggingMatchType::Fuzzy,
            source_keys: vec![query_key.to_string()],
            deterministic_keys: vec![],
            name_path,
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::tagging::types::TaggingMatchType;

    fn cand(id: Uuid, name: &str, score: f32, mt: TaggingMatchType, key: &str) -> NodeCandidate {
        NodeCandidate {
            id,
            name: name.to_string(),
            tree_id: Uuid::new_v4(),
            path: "a".into(),
            depth: 1,
            score,
            match_type: mt,
            source_keys: vec![key.to_string()],
            deterministic_keys: if mt.is_deterministic() {
                vec![key.to_string()]
            } else {
                vec![]
            },
            name_path: name.to_string(),
        }
    }

    #[test]
    fn vector_score_never_exceeds_cap() {
        assert_eq!(cap_vector_score(1.0), VECTOR_SCORE_CAP);
        assert_eq!(cap_vector_score(0.55), 0.55);
        assert_eq!(cap_vector_score(-0.1), 0.0);
    }

    #[test]
    fn exact_not_overwritten_by_vector_fuzzy() {
        let id = Uuid::new_v4();
        let mut merged = HashMap::new();
        merge_node_candidate(
            &mut merged,
            cand(id, "交集的概念及运算", 1.0, TaggingMatchType::Exact, "交集"),
        );
        merge_node_candidate(
            &mut merged,
            cand(
                id,
                "交集的概念及运算",
                0.80,
                TaggingMatchType::Fuzzy,
                "两个集合的公共元素",
            ),
        );
        let got = merged.get(&id).unwrap();
        assert_eq!(got.match_type, TaggingMatchType::Exact);
        assert_eq!(got.score, 1.0);
        assert!(got.source_keys.iter().any(|k| k == "交集"));
        assert!(got.source_keys.iter().any(|k| k == "两个集合的公共元素"));
    }

    #[test]
    fn vector_only_is_fuzzy_capped() {
        let id = Uuid::new_v4();
        let mut merged = HashMap::new();
        merge_node_candidate(
            &mut merged,
            cand(
                id,
                "两个集合的公共元素",
                cap_vector_score(0.93),
                TaggingMatchType::Fuzzy,
                "交集",
            ),
        );
        let got = merged.get(&id).unwrap();
        assert_eq!(got.match_type, TaggingMatchType::Fuzzy);
        assert!(got.score <= VECTOR_SCORE_CAP);
        assert_eq!(got.score, 0.80);
    }

    #[test]
    fn both_fuzzy_keeps_higher_score() {
        let id = Uuid::new_v4();
        let mut merged = HashMap::new();
        merge_node_candidate(
            &mut merged,
            cand(id, "交集", 0.65, TaggingMatchType::Fuzzy, "交"),
        );
        merge_node_candidate(
            &mut merged,
            cand(id, "交集", 0.80, TaggingMatchType::Fuzzy, "交集"),
        );
        let got = merged.get(&id).unwrap();
        assert_eq!(got.match_type, TaggingMatchType::Fuzzy);
        assert_eq!(got.score, 0.80);
        assert_eq!(got.source_keys.len(), 2);
    }
}
