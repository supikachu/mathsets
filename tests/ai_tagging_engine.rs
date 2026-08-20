//! 智能打标引擎：召回规则、上限、适配器一致性（需 DATABASE_URL_TEST�?

use mathset::ai::tagging::repository::{match_nodes, recall_nodes};
use mathset::ai::tagging::{
    run_tagging, signals_from_parsed, TaggingContext, TaggingDimension, TaggingInput,
    TaggingPolicy, ENGINE_VERSION,
};
use mathset::ai::types::{ParsedQuestion, SolutionMethod};
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

async fn insert_space(pool: &sqlx::PgPool) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO spaces (id, kind, name, owner_user_id, settings, created_at, updated_at) VALUES ($1, 'team', $2, NULL, '{}'::jsonb, NOW(), NOW())",
    )
    .bind(id)
    .bind(unique_name("e7_space"))
    .execute(pool)
    .await
    .expect("insert space");
    id
}

async fn insert_tree(pool: &sqlx::PgPool, kind: &str, space_id: Option<Uuid>) -> Uuid {
    let id = Uuid::new_v4();
    let code = unique_name("e7_tree");
    sqlx::query(
        "INSERT INTO knowledge_trees (id, code, name, kind, space_id, is_active, created_at, updated_at) VALUES ($1, $2, $3, $4::knowledge_tree_kind, $5, TRUE, NOW(), NOW())",
    )
    .bind(id)
    .bind(&code)
    .bind(&code)
    .bind(kind)
    .bind(space_id)
    .execute(pool)
    .await
    .expect("insert tree");
    id
}

async fn insert_node(
    pool: &sqlx::PgPool,
    tree_id: Uuid,
    name: &str,
    path: &str,
    parent_id: Option<Uuid>,
    depth: i16,
) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query(
        r#"
        INSERT INTO knowledge_nodes (id, tree_id, parent_id, path, depth, name, aliases, is_active, status, source, created_at, updated_at)
        VALUES ($1, $2, $3, $4::ltree, $5, $6, '[]'::jsonb, TRUE, 'active', 'system', NOW(), NOW())
        "#,
    )
    .bind(id)
    .bind(tree_id)
    .bind(parent_id)
    .bind(path)
    .bind(depth)
    .bind(name)
    .execute(pool)
    .await
    .expect("insert node");
    id
}

fn offline_policy() -> TaggingPolicy {
    TaggingPolicy {
        run_llm_extract: false,
        run_llm_converge: false,
        fail_on_persist: false,
        ..TaggingPolicy::default()
    }
}

fn parsed(knowledge: Vec<String>, chapter: Vec<String>, methods: Vec<String>) -> ParsedQuestion {
    ParsedQuestion {
        question_type: "solution".into(),
        sub_type: None,
        difficulty: Some("medium".into()),
        stem: "引擎测试题干".into(),
        options: None,
        correct_answer: None,
        analysis: vec![],
        knowledge_points: knowledge,
        confidence: 0.9,
        warnings: vec![],
        image_placeholders: vec![],
        image_urls: vec![],
        kp_matches: vec![],
        question_no: None,
        display_order: None,
        score: None,
        chapter_path: chapter,
        solution_methods: methods
            .into_iter()
            .map(|name| SolutionMethod {
                name,
                confidence: Some(0.9),
            })
            .collect(),
    }
}

#[tokio::test]
async fn test_exact_alias_beats_fuzzy_and_excludes_inactive() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let tree = insert_tree(&pool, "knowledge", None).await;
    let exact_name = unique_name("精确知识�?);
    let alias_name = unique_name("别名�?);
    let alias_raw = unique_name("别名命中");
    let inactive_name = unique_name("停用节点");
    let exact_id = insert_node(&pool, tree, &exact_name, &ltree_seg(), None, 1).await;
    let alias_id = insert_node(&pool, tree, &alias_name, &ltree_seg(), None, 1).await;
    sqlx::query(
        "UPDATE knowledge_nodes SET aliases = jsonb_build_array(jsonb_build_object('alias', $2::text)) WHERE id = $1",
    )
    .bind(alias_id)
    .bind(&alias_raw)
    .execute(&pool)
    .await
    .unwrap();
    let inactive_id = insert_node(&pool, tree, &inactive_name, &ltree_seg(), None, 1).await;
    sqlx::query("UPDATE knowledge_nodes SET is_active = FALSE WHERE id = $1")
        .bind(inactive_id)
        .execute(&pool)
        .await
        .unwrap();

    let recs = recall_nodes(
        &pool,
        &[exact_name.clone(), alias_raw.clone(), inactive_name.clone()],
        TaggingDimension::Knowledge,
        &offline_policy(),
        None,
        None,
    )
    .await
    .expect("recall");
    assert!(recs
        .iter()
        .any(|c| c.id == exact_id && c.match_type.as_str() == "exact"));
    assert!(recs
        .iter()
        .any(|c| c.id == alias_id && c.match_type.as_str() == "alias"));
    assert!(recs.iter().all(|c| c.id != inactive_id));
}

#[tokio::test]
async fn test_merged_deprecated_rejected_and_canonical_excluded() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let tree = insert_tree(&pool, "knowledge", None).await;
    let live_name = unique_name("活节�?);
    let merged_name = unique_name("已合�?);
    let deprecated_name = unique_name("已弃�?);
    let rejected_name = unique_name("已拒�?);
    let live = insert_node(&pool, tree, &live_name, &ltree_seg(), None, 1).await;
    let merged = insert_node(&pool, tree, &merged_name, &ltree_seg(), None, 1).await;
    let deprecated = insert_node(&pool, tree, &deprecated_name, &ltree_seg(), None, 1).await;
    let rejected = insert_node(&pool, tree, &rejected_name, &ltree_seg(), None, 1).await;
    sqlx::query("UPDATE knowledge_nodes SET status = 'merged', canonical_id = $2 WHERE id = $1")
        .bind(merged)
        .bind(live)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE knowledge_nodes SET status = 'deprecated' WHERE id = $1")
        .bind(deprecated)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("UPDATE knowledge_nodes SET status = 'rejected' WHERE id = $1")
        .bind(rejected)
        .execute(&pool)
        .await
        .unwrap();

    let recs = recall_nodes(
        &pool,
        &[
            merged_name.clone(),
            live_name.clone(),
            deprecated_name.clone(),
            rejected_name.clone(),
        ],
        TaggingDimension::Knowledge,
        &offline_policy(),
        None,
        None,
    )
    .await
    .expect("recall");
    assert!(recs.iter().any(|c| c.id == live));
    assert!(recs.iter().all(|c| c.id != merged));
    assert!(recs.iter().all(|c| c.id != deprecated));
    assert!(recs.iter().all(|c| c.id != rejected));
}

#[tokio::test]
async fn test_chapter_allows_parent_knowledge_and_pattern_leaf_only() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let ch_tree = insert_tree(&pool, "chapter", None).await;
    let kn_tree = insert_tree(&pool, "knowledge", None).await;
    let pat_tree = insert_tree(&pool, "ability", None).await;
    let parent_name = unique_name("父章�?);
    let child_name = unique_name("叶子章节");
    let kn_parent = unique_name("知识�?);
    let kn_leaf = unique_name("知识�?);
    let pat_parent = unique_name("专题�?);
    let pat_leaf = unique_name("专题�?);

    let ch_pseg = ltree_seg();
    let ch_parent = insert_node(&pool, ch_tree, &parent_name, &ch_pseg, None, 0).await;
    let _ch_child = insert_node(
        &pool,
        ch_tree,
        &child_name,
        &format!("{ch_pseg}.{}", ltree_seg()),
        Some(ch_parent),
        1,
    )
    .await;

    let kn_pseg = ltree_seg();
    let kn_p = insert_node(&pool, kn_tree, &kn_parent, &kn_pseg, None, 0).await;
    insert_node(
        &pool,
        kn_tree,
        &kn_leaf,
        &format!("{kn_pseg}.{}", ltree_seg()),
        Some(kn_p),
        1,
    )
    .await;

    let pat_pseg = ltree_seg();
    let pat_p = insert_node(&pool, pat_tree, &pat_parent, &pat_pseg, None, 0).await;
    insert_node(
        &pool,
        pat_tree,
        &pat_leaf,
        &format!("{pat_pseg}.{}", ltree_seg()),
        Some(pat_p),
        1,
    )
    .await;

    let ch_recs = recall_nodes(
        &pool,
        &[parent_name.clone()],
        TaggingDimension::Chapter,
        &offline_policy(),
        None,
        None,
    )
    .await
    .expect("chapter recall");
    assert!(
        ch_recs.iter().any(|c| c.id == ch_parent),
        "章节应能命中父节�? {ch_recs:?}"
    );

    let kn_recs = recall_nodes(
        &pool,
        &[kn_parent.clone()],
        TaggingDimension::Knowledge,
        &offline_policy(),
        None,
        None,
    )
    .await
    .expect("knowledge recall");
    assert!(
        kn_recs.iter().all(|c| c.id != kn_p),
        "知识点不应命中仍有子节点的父节点: {kn_recs:?}"
    );

    let pat_recs = recall_nodes(
        &pool,
        &[pat_parent.clone()],
        TaggingDimension::Pattern,
        &offline_policy(),
        None,
        None,
    )
    .await
    .expect("pattern recall");
    assert!(
        pat_recs.iter().all(|c| c.id != pat_p),
        "题型专题不应命中仍有子节点的父节�? {pat_recs:?}"
    );
}

#[tokio::test]
async fn test_space_isolation() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let space_a = insert_space(&pool).await;
    let space_b = insert_space(&pool).await;
    let tree = insert_tree(&pool, "knowledge", Some(space_a)).await;
    let name = unique_name("空间隔离节点");
    let node_id = insert_node(&pool, tree, &name, &ltree_seg(), None, 1).await;

    let in_a = recall_nodes(
        &pool,
        &[name.clone()],
        TaggingDimension::Knowledge,
        &offline_policy(),
        Some(space_a),
        None,
    )
    .await
    .expect("space a");
    assert!(in_a.iter().any(|c| c.id == node_id));

    let in_b = recall_nodes(
        &pool,
        &[name.clone()],
        TaggingDimension::Knowledge,
        &offline_policy(),
        Some(space_b),
        None,
    )
    .await
    .expect("space b");
    assert!(in_b.iter().all(|c| c.id != node_id));

    let global = recall_nodes(
        &pool,
        &[name.clone()],
        TaggingDimension::Knowledge,
        &offline_policy(),
        None,
        None,
    )
    .await
    .expect("global");
    assert!(
        global.iter().all(|c| c.id != node_id),
        "无空间上下文不应命中空间树节�?
    );
}

#[tokio::test]
async fn test_engine_no_silent_top1_and_max_limit() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let tree = insert_tree(&pool, "knowledge", None).await;
    let mut names = Vec::new();
    for i in 0..4 {
        let n = unique_name(&format!("上限{i}"));
        insert_node(&pool, tree, &n, &ltree_seg(), None, 1).await;
        names.push(n);
    }
    let q = parsed(names.clone(), vec![], vec![]);
    let mut policy = offline_policy();
    policy.max_knowledge = 3;
    let suggestion = run_tagging(
        &pool,
        None,
        None,
        TaggingInput::Parsed(Box::new(q)),
        &TaggingContext::default(),
        &policy,
    )
    .await
    .expect("run_tagging");
    let kn: Vec<_> = suggestion
        .matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::Knowledge)
        .collect();
    assert_eq!(kn.len(), 3, "知识点上限应�?3: {:?}", kn);
    assert_eq!(suggestion.engine_version, ENGINE_VERSION);
    assert!(suggestion
        .unmatched
        .iter()
        .any(|u| u.dimension == TaggingDimension::Knowledge));

    let fuzzy_key = format!("{}变式", names[0]);
    let q2 = parsed(vec![fuzzy_key], vec![], vec![]);
    let suggestion2 = run_tagging(
        &pool,
        None,
        None,
        TaggingInput::Parsed(Box::new(q2)),
        &TaggingContext::default(),
        &policy,
    )
    .await
    .expect("run_tagging fuzzy");
    let auto: Vec<_> = suggestion2
        .matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::Knowledge)
        .collect();
    assert!(
        auto.is_empty(),
        "关闭收敛时不应静默接�?fuzzy Top1: {:?}",
        suggestion2.matches
    );
    assert!(suggestion2.needs_review);
}

#[tokio::test]
async fn test_parsed_adapter_matches_direct_recall() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let kn_tree = insert_tree(&pool, "knowledge", None).await;
    let ch_tree = insert_tree(&pool, "chapter", None).await;
    let kn_name = unique_name("一致知识点");
    let ch_name = unique_name("一致章�?);
    let method_name = unique_name("一致方�?);
    let kn_id = insert_node(&pool, kn_tree, &kn_name, &ltree_seg(), None, 1).await;
    let ch_id = insert_node(&pool, ch_tree, &ch_name, &ltree_seg(), None, 1).await;
    sqlx::query(
        r#"
        INSERT INTO tags (id, name, category, aliases, use_count, is_active, created_at)
        VALUES ($1, $2, 'method', '[]'::jsonb, 0, TRUE, NOW())
        "#,
    )
    .bind(Uuid::new_v4())
    .bind(&method_name)
    .execute(&pool)
    .await
    .expect("insert method tag");

    let q = parsed(
        vec![kn_name.clone()],
        vec!["教材".into(), ch_name.clone()],
        vec![method_name.clone()],
    );
    let signals = signals_from_parsed(&q);
    assert_eq!(signals.chapter_keys, vec![ch_name.clone()]);
    assert_eq!(signals.knowledge_keys, vec![kn_name.clone()]);
    assert_eq!(signals.method_keys, vec![method_name.clone()]);

    let suggestion = run_tagging(
        &pool,
        None,
        None,
        TaggingInput::Parsed(Box::new(q)),
        &TaggingContext::default(),
        &offline_policy(),
    )
    .await
    .expect("run_tagging");

    let rec_kn = recall_nodes(
        &pool,
        &signals.knowledge_keys,
        TaggingDimension::Knowledge,
        &offline_policy(),
        None,
        None,
    )
    .await
    .unwrap();
    let rec_ch = recall_nodes(
        &pool,
        &signals.chapter_keys,
        TaggingDimension::Chapter,
        &offline_policy(),
        None,
        None,
    )
    .await
    .unwrap();

    let engine_kn: Vec<_> = suggestion
        .matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::Knowledge)
        .map(|m| m.target_id)
        .collect();
    let engine_ch: Vec<_> = suggestion
        .matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::Chapter)
        .map(|m| m.target_id)
        .collect();
    assert_eq!(engine_kn, vec![kn_id]);
    assert_eq!(engine_ch, vec![ch_id]);
    assert_eq!(
        rec_kn
            .iter()
            .filter(|c| c.match_type.is_deterministic())
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        engine_kn
    );
    assert_eq!(
        rec_ch
            .iter()
            .filter(|c| c.match_type.is_deterministic())
            .map(|c| c.id)
            .collect::<Vec<_>>(),
        engine_ch
    );
    assert!(suggestion
        .matches
        .iter()
        .any(|m| m.dimension == TaggingDimension::Method && m.target_name == method_name));
}

#[tokio::test]
async fn test_content_adapter_offline_does_not_invent_matches() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let tree = insert_tree(&pool, "knowledge", None).await;
    let name = unique_name("题文不应直接命中");
    insert_node(&pool, tree, &name, &ltree_seg(), None, 1).await;
    let suggestion = run_tagging(
        &pool,
        None,
        None,
        TaggingInput::Content {
            content: format!("已知{name}，求最值�?),
        },
        &TaggingContext::default(),
        &offline_policy(),
    )
    .await
    .expect("content tagging");
    assert!(
        suggestion.matches.is_empty(),
        "关闭提取�?Content 适配器不应凭题文静默召回: {:?}",
        suggestion.matches
    );
}

#[tokio::test]
async fn test_old_top1_includes_fuzzy_engine_does_not() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let tree = insert_tree(&pool, "knowledge", None).await;
    let exact = unique_name("二次函数最值问�?);
    insert_node(&pool, tree, &exact, &ltree_seg(), None, 1).await;
    let key = format!("{exact}变式");
    let (old_matched, _) = match_nodes(&pool, &[key.clone()], None, "knowledge")
        .await
        .expect("match_nodes");
    let suggestion = run_tagging(
        &pool,
        None,
        None,
        TaggingInput::Parsed(Box::new(parsed(vec![key], vec![], vec![]))),
        &TaggingContext::default(),
        &offline_policy(),
    )
    .await
    .expect("engine");
    let engine_kn: Vec<_> = suggestion
        .matches
        .iter()
        .filter(|m| m.dimension == TaggingDimension::Knowledge)
        .collect();
    if !old_matched.is_empty() && old_matched[0].match_type != "exact" {
        assert!(
            engine_kn.is_empty(),
            "�?Top1 可收�?fuzzy，引擎关闭收敛后不应自动收下"
        );
        assert!(!suggestion.unmatched.is_empty());
    }
}

#[tokio::test]
async fn test_suggestion_serde_roundtrip() {
    let raw = serde_json::json!({
        "suggestion_id": Uuid::new_v4(),
        "engine_version": ENGINE_VERSION,
        "input_hash": "abc",
        "needs_review": true,
        "matches": [{
            "dimension": "knowledge",
            "target_type": "knowledge_node",
            "ai_name": "二次函数",
            "target_id": Uuid::new_v4(),
            "target_name": "二次函数",
            "tree_id": Uuid::new_v4(),
            "path": "a.b",
            "depth": 1,
            "category": null,
            "score": 1.0,
            "match_type": "exact"
        }],
        "unmatched": [{
            "id": "u1",
            "dimension": "method",
            "target_type": "tag",
            "raw_name": "数形结合变体",
            "normalized_name": "数形结合变体",
            "confidence": null,
            "reason": "no_deterministic_match",
            "eligible_for_candidate": true
        }],
        "difficulty": 3,
        "question_type": "solution",
        "grade_level": "grade_12",
        "cognitive_level": "apply"
    });
    let s: mathset::ai::tagging::TaggingSuggestion = serde_json::from_value(raw).unwrap();
    assert_eq!(s.difficulty, Some(3));
    assert_eq!(s.grade_level.as_deref(), Some("grade_12"));
    assert_eq!(s.cognitive_level.as_deref(), Some("apply"));
    assert_eq!(s.matches[0].dimension, TaggingDimension::Knowledge);
    assert_eq!(s.unmatched[0].dimension, TaggingDimension::Method);
    let back = serde_json::to_value(&s).unwrap();
    assert_eq!(back["engine_version"], ENGINE_VERSION);
}

#[tokio::test]
async fn test_semantic_recall_chapter_key_contained_in_textbook_title() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let tree = insert_tree(&pool, "chapter", None).await;
    let marker = unique_name("e7sem");
    let chapter_name = format!("第一�?{marker}与常用逻辑用语");
    let section_name = format!("1.3 {marker}的基本运�?);
    let pseg = ltree_seg();
    let chapter_id = insert_node(&pool, tree, &chapter_name, &pseg, None, 0).await;
    let _section_id = insert_node(
        &pool,
        tree,
        &section_name,
        &format!("{pseg}.{}", ltree_seg()),
        Some(chapter_id),
        1,
    )
    .await;

    let mut policy = offline_policy();
    policy.recall_limit_chapter = 80;
    let recs = recall_nodes(
        &pool,
        &[marker.clone()],
        TaggingDimension::Chapter,
        &policy,
        None,
        None,
    )
    .await
    .expect("semantic chapter recall");

    let hit = recs
        .iter()
        .find(|c| c.id == chapter_id)
        .unwrap_or_else(|| panic!("短关键词应召回教材式章节�?{chapter_name}，实�?{recs:?}"));
    assert_eq!(hit.match_type.as_str(), "fuzzy", "包含命中不得升为 exact");
    assert!(
        recs.iter().any(|c| c.name == section_name),
        "带节号的子章节也应进入模糊候�? {recs:?}"
    );
}

#[tokio::test]
async fn test_semantic_recall_knowledge_leaf_from_paraphrase() {
    let Some(pool) = test_pool().await else {
        eprintln!("跳过：未配置 DATABASE_URL_TEST");
        return;
    };
    let tree = insert_tree(&pool, "knowledge", None).await;
    let marker = unique_name("e7ik");
    let set_name = format!("{marker}集合");
    let cap_name = format!("{marker}交集");
    let leaf_name = format!("{marker}交集的概念及运算");
    let pseg = ltree_seg();
    let set_id = insert_node(&pool, tree, &set_name, &pseg, None, 0).await;
    let cap_id = insert_node(
        &pool,
        tree,
        &cap_name,
        &format!("{pseg}.{}", ltree_seg()),
        Some(set_id),
        1,
    )
    .await;
    let cap_path: String = sqlx::query_scalar("SELECT path::text FROM knowledge_nodes WHERE id = $1")
        .bind(cap_id)
        .fetch_one(&pool)
        .await
        .expect("cap path");
    let leaf_id = insert_node(
        &pool,
        tree,
        &leaf_name,
        &format!("{cap_path}.{}", ltree_seg()),
        Some(cap_id),
        2,
    )
    .await;

    let mut policy = offline_policy();
    policy.recall_limit_knowledge = 80;
    let key = format!("{marker}集合的交集运�?);
    let recs = recall_nodes(
        &pool,
        &[key],
        TaggingDimension::Knowledge,
        &policy,
        None,
        None,
    )
    .await
    .expect("semantic knowledge recall");

    let hit = recs.iter().find(|c| c.id == leaf_id).unwrap_or_else(|| {
        panic!("「集合的交集运算」类转写应召回叶子「交集的概念及运算」变体，实际 {recs:?}")
    });
    assert_eq!(hit.match_type.as_str(), "fuzzy");
    assert!(
        hit.name_path.contains(&set_name) && hit.name_path.contains(&leaf_name),
        "收敛菜单需要祖先路�? {}",
        hit.name_path
    );
    assert!(
        recs.iter().all(|c| c.id != set_id && c.id != cap_id),
        "知识点不得召回非叶子: {recs:?}"
    );
}
