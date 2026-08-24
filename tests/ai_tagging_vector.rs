//! 向量召回：无 pgvector 则 skip；有扩展时用假向量测并集与「不得升 exact」

use std::collections::HashMap;

use mathset::ai::embedding::{embeddings_table_ready, format_vector, EMBEDDING_DIM};
use mathset::ai::tagging::repository::NodeCandidate;
use mathset::ai::tagging::types::TaggingMatchType;
use mathset::ai::tagging::vector::{
    cap_vector_score, merge_node_candidate, search_nodes_by_embedding, VECTOR_SCORE_CAP,
};
use mathset::ai::tagging::ENGINE_VERSION;
use mathset::db;
use uuid::Uuid;

async fn test_pool() -> Option<sqlx::PgPool> {
    let database_url = mathset::testing::database_url()?;
    let pool = db::create_pool(&database_url, 5).await;
    db::run_migrations(&pool).await;
    Some(pool)
}

fn unique_name(prefix: &str) -> String {
    format!("{prefix}_{}", Uuid::new_v4().simple())
}

fn ltree_seg() -> String {
    format!("n{}", Uuid::new_v4().simple())
}

fn unit_vector(hot: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; EMBEDDING_DIM];
    v[hot] = 1.0;
    v
}

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
fn engine_version_is_v4() {
    assert_eq!(ENGINE_VERSION, "tagging-v4");
}

#[test]
fn exact_not_overwritten_by_vector_hit() {
    let id = Uuid::new_v4();
    let mut merged = HashMap::new();
    merge_node_candidate(
        &mut merged,
        cand(id, "交集的概念及运算", 1.0, TaggingMatchType::Exact, "交集"),
    );
    merge_node_candidate(
        &mut merged,
        cand(id, "交集的概念及运算", 0.80, TaggingMatchType::Fuzzy, "公共元素"),
    );
    let got = merged.get(&id).unwrap();
    assert_eq!(got.match_type, TaggingMatchType::Exact);
    assert_eq!(got.score, 1.0);
    assert_eq!(got.source_keys.len(), 2);
}

#[test]
fn vector_only_is_fuzzy_and_capped() {
    let id = Uuid::new_v4();
    let mut merged = HashMap::new();
    merge_node_candidate(
        &mut merged,
        cand(
            id,
            "两个集合的公共元素",
            cap_vector_score(0.97),
            TaggingMatchType::Fuzzy,
            "交集",
        ),
    );
    let got = merged.get(&id).unwrap();
    assert_eq!(got.match_type, TaggingMatchType::Fuzzy);
    assert!(got.score <= VECTOR_SCORE_CAP);
    assert_eq!(got.score, VECTOR_SCORE_CAP);
}

#[tokio::test]
async fn vector_ann_with_fake_embeddings_skips_without_pgvector() {
    let Some(pool) = test_pool().await else {
        eprintln!("skip: DATABASE_URL_TEST 未配置");
        return;
    };
    if !embeddings_table_ready(&pool).await {
        eprintln!("skip: knowledge_node_embeddings 不存在（未安装 pgvector）");
        return;
    }

    let space_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO spaces (id, kind, name, owner_user_id, settings, created_at, updated_at) VALUES ($1, 'team', $2, NULL, '{}'::jsonb, NOW(), NOW())",
    )
    .bind(space_id)
    .bind(unique_name("vec_space"))
    .execute(&pool)
    .await
    .expect("insert space");

    let tree_id = Uuid::new_v4();
    let code = unique_name("vec_tree");
    sqlx::query(
        "INSERT INTO knowledge_trees (id, code, name, kind, space_id, is_active, created_at, updated_at) VALUES ($1, $2, $3, 'knowledge'::knowledge_tree_kind, $4, TRUE, NOW(), NOW())",
    )
    .bind(tree_id)
    .bind(&code)
    .bind(&code)
    .bind(space_id)
    .execute(&pool)
    .await
    .expect("insert tree");

    let target_id = Uuid::new_v4();
    let noise_id = Uuid::new_v4();
    let target_path = ltree_seg();
    let noise_path = ltree_seg();
    sqlx::query(
        r#"
        INSERT INTO knowledge_nodes (id, tree_id, parent_id, path, depth, name, aliases, is_active, status, source, created_at, updated_at)
        VALUES ($1, $2, NULL, $3::ltree, 0, $4, '[]'::jsonb, TRUE, 'active', 'system', NOW(), NOW())
        "#,
    )
    .bind(target_id)
    .bind(tree_id)
    .bind(&target_path)
    .bind("两个集合的公共元素")
    .execute(&pool)
    .await
    .expect("insert target node");
    sqlx::query(
        r#"
        INSERT INTO knowledge_nodes (id, tree_id, parent_id, path, depth, name, aliases, is_active, status, source, created_at, updated_at)
        VALUES ($1, $2, NULL, $3::ltree, 0, $4, '[]'::jsonb, TRUE, 'active', 'system', NOW(), NOW())
        "#,
    )
    .bind(noise_id)
    .bind(tree_id)
    .bind(&noise_path)
    .bind("正弦函数的图象")
    .execute(&pool)
    .await
    .expect("insert noise node");

    sqlx::query(
        r#"
        INSERT INTO knowledge_node_embeddings (node_id, content_hash, embedding, updated_at)
        VALUES ($1, 'fake-target', $2::vector, NOW())
        "#,
    )
    .bind(target_id)
    .bind(format_vector(&unit_vector(0)))
    .execute(&pool)
    .await
    .expect("insert target embedding");
    sqlx::query(
        r#"
        INSERT INTO knowledge_node_embeddings (node_id, content_hash, embedding, updated_at)
        VALUES ($1, 'fake-noise', $2::vector, NOW())
        "#,
    )
    .bind(noise_id)
    .bind(format_vector(&unit_vector(10)))
    .execute(&pool)
    .await
    .expect("insert noise embedding");

    let hits = search_nodes_by_embedding(
        &pool,
        &unit_vector(0),
        "交集",
        "knowledge",
        false,
        Some(space_id),
        None,
        None,
        0.3,
    )
    .await
    .expect("vector search");

    assert!(
        hits.iter().any(|c| c.id == target_id),
        "查询向量应召回语义相近节点，实际: {:?}",
        hits.iter().map(|c| &c.name).collect::<Vec<_>>()
    );
    let target = hits.iter().find(|c| c.id == target_id).unwrap();
    assert_eq!(target.match_type, TaggingMatchType::Fuzzy);
    assert!(target.score <= VECTOR_SCORE_CAP);

    let mut merged = HashMap::new();
    merge_node_candidate(
        &mut merged,
        cand(
            target_id,
            "两个集合的公共元素",
            1.0,
            TaggingMatchType::Exact,
            "两个集合的公共元素",
        ),
    );
    merge_node_candidate(&mut merged, target.clone());
    let kept = merged.get(&target_id).unwrap();
    assert_eq!(kept.match_type, TaggingMatchType::Exact);
    assert_eq!(kept.score, 1.0);
}
