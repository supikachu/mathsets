//! 知识树数据初始化脚本（全量覆盖导入）
//!
//! 用途：将两份全新的树形 JSON 数据覆盖导入到 PostgreSQL：
//!   - temp/labels1_tree.json → 高中数学知识点树（kind = 'knowledge'）
//!   - temp/labels2_tree.json → 高中数学教材章节树（kind = 'chapter'）
//!
//! 安全特性：
//!   1. 单一全局事务：清空 + 插入全部包裹在同一个 BEGIN/COMMIT 中，
//!      任意步骤失败自动 ROLLBACK，旧数据完整保留，绝不出现"真空瘫痪"。
//!   2. 显式按依赖顺序清空：question_knowledge_nodes → knowledge_nodes → knowledge_trees
//!   3. questions 表物理题目完全不受影响（FK 设计天然保护：
//!      question_knowledge_nodes.node_id ON DELETE CASCADE 只删关联，不删题目）
//!   4. LTREE path 段使用 REPLACE(uuid, '-', '_')，规避 ltree 不允许 '-' 字符的限制
//!   5. sort_order 使用 children 数组索引，确保前端渲染顺序与 JSON 完全一致
//!
//! 运行方式：
//!   cargo run --bin import_trees
//!
//! 默认 JSON 文件路径相对于 CARGO_MANIFEST_DIR（项目根目录）。

use std::path::PathBuf;
use std::pin::Pin;

use serde::Deserialize;
use uuid::Uuid;

use mathset::config::AppConfig;
use mathset::db;

// ===========================================================================
// JSON 数据结构
// ===========================================================================

/// JSON 节点：递归 { text, children } 结构
#[derive(Debug, Deserialize)]
struct JsonNode {
    text: String,
    #[serde(default)]
    children: Vec<JsonNode>,
}

// ===========================================================================
// 树元数据常量
// ===========================================================================

/// 单棵树的导入配置（配置驱动，便于扩展）
struct TreeSpec {
    /// JSON 文件相对路径（相对于 CARGO_MANIFEST_DIR）
    path: &'static str,
    /// 树类型：'knowledge' | 'ability' | 'chapter'
    kind: &'static str,
    /// 树 code，命名规则：{subject}_{mode}_{stage}
    /// 如 'math_knowledge_high'（高中数学知识点树）
    code: &'static str,
    /// 树名称（显示用）
    name: &'static str,
    /// 树描述
    desc: &'static str,
    /// 当一个 JSON 文件含多个顶级根时，用 root_filter 指定要导入的根 text
    /// None = 导入文件内所有顶级节点（每个作为该树 depth=0 的根）
    root_filter: Option<&'static str>,
}

/// 9 棵树的统一配置（高中数学 3 + 初中数学 3 + 高中物理 3）
/// code 命名规则与前端 KnowledgeTreeNav.vue 的 expectedCode 完全对齐
const TREE_SPECS: &[TreeSpec] = &[
    // ─── 高中数学（labels1 含知识点+方法两个根，需 root_filter 拆分） ───
    TreeSpec {
        path: "temp/labels1_tree.json",
        kind: "knowledge",
        code: "math_knowledge_high",
        name: "高中数学知识点树",
        desc: "高中数学全量知识点树",
        root_filter: Some("高中数学知识点树"),
    },
    TreeSpec {
        path: "temp/labels1_tree.json",
        kind: "ability",
        code: "math_method_high",
        name: "高中数学方法维度库",
        desc: "高中数学解题方法维度库",
        root_filter: Some("高中数学方法维度库"),
    },
    TreeSpec {
        path: "temp/labels2_tree.json",
        kind: "chapter",
        code: "math_chapter_high",
        name: "高中数学教材章节树",
        desc: "高中数学教材章节树",
        root_filter: None,
    },
    // ─── 初中数学（m-labels-*2_tree.json） ───
    TreeSpec {
        path: "temp/m-labels-chapter2_tree.json",
        kind: "chapter",
        code: "math_chapter_junior",
        name: "初中数学教材章节树",
        desc: "初中数学教材章节树",
        root_filter: None,
    },
    TreeSpec {
        path: "temp/m-labels-knowledge2_tree.json",
        kind: "knowledge",
        code: "math_knowledge_junior",
        name: "初中数学知识点树",
        desc: "初中数学全量知识点树",
        root_filter: None,
    },
    TreeSpec {
        path: "temp/m-labels-method2_tree.json",
        kind: "ability",
        code: "math_method_junior",
        name: "初中数学方法维度库",
        desc: "初中数学解题方法维度库",
        root_filter: None,
    },
    // ─── 高中物理（p-labels-*_tree1.json） ───
    TreeSpec {
        path: "temp/p-labels-chapter_tree1.json",
        kind: "chapter",
        code: "physics_chapter_high",
        name: "高中物理教材章节树",
        desc: "高中物理教材章节树",
        root_filter: None,
    },
    TreeSpec {
        path: "temp/p-labels-knowledge_tree1.json",
        kind: "knowledge",
        code: "physics_knowledge_high",
        name: "高中物理知识点树",
        desc: "高中物理全量知识点树",
        root_filter: None,
    },
    TreeSpec {
        path: "temp/p-labels-method_tree1.json",
        kind: "ability",
        code: "physics_method_high",
        name: "高中物理方法维度库",
        desc: "高中物理解题方法维度库",
        root_filter: None,
    },
];

// ===========================================================================
// 入口
// ===========================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 加载 .env + 初始化日志
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "import_trees=info,mathset=warn".into()),
        )
        .init();

    tracing::info!("🚀 知识树数据初始化脚本启动");

    // 加载配置 + 连接数据库（连接池仅需 5 个连接，脚本场景足够）
    let config = AppConfig::from_env();
    let pool = db::create_pool(&config.database_url, 5).await;
    tracing::info!("✅ 数据库连接成功");

    // 预读所有 JSON 文件（去重，避免 labels1 被读两次）
    use std::collections::HashMap;
    let mut json_cache: HashMap<&'static str, Vec<JsonNode>> = HashMap::new();
    for spec in TREE_SPECS {
        if json_cache.contains_key(spec.path) {
            continue;
        }
        let path = locate_json(spec.path)?;
        tracing::info!("📖 读取 {:?}:", path);
        let nodes: Vec<JsonNode> = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
        tracing::info!("   → 解析到 {} 个顶级节点", nodes.len());
        json_cache.insert(spec.path, nodes);
    }

    // ─── 全局事务：清空 + 插入 ───────────────────────────────────────────
    tracing::info!("🔒 开启全局事务（清空 + 插入，失败自动 ROLLBACK）");
    let mut tx = pool.begin().await?;

    // ─── Step 1: 清空旧数据（按 FK 依赖顺序） ────────────────────────────
    tracing::info!("🧹 正在清理旧数据...");

    let r = sqlx::query("DELETE FROM question_knowledge_nodes")
        .execute(&mut *tx)
        .await?;
    tracing::info!("   → question_knowledge_nodes 已清空: {} 行", r.rows_affected());

    let r = sqlx::query("DELETE FROM knowledge_nodes")
        .execute(&mut *tx)
        .await?;
    tracing::info!("   → knowledge_nodes 已清空: {} 行", r.rows_affected());

    let r = sqlx::query("DELETE FROM knowledge_trees")
        .execute(&mut *tx)
        .await?;
    tracing::info!("   → knowledge_trees 已清空: {} 行", r.rows_affected());

    tracing::info!("✅ 旧数据清理完成（questions 表未受影响）");

    // ─── Step 2: 遍历 TREE_SPECS，建树 + 递归插入 ─────────────────────
    // 配置驱动：每棵树按 spec.kind/code/name 建元数据，再递归插入节点
    // root_filter 用于处理 labels1 单文件多根的情况
    let mut total_count: usize = 0;
    for (idx, spec) in TREE_SPECS.iter().enumerate() {
        let nodes = &json_cache[spec.path];
        // 按 root_filter 筛选根节点
        let roots: Vec<&JsonNode> = match spec.root_filter {
            Some(filter) => nodes.iter().filter(|n| n.text == filter).collect(),
            None => nodes.iter().collect(),
        };
        if roots.is_empty() {
            tracing::warn!(
                "⚠️ [{}] 未匹配到根节点 (filter={:?})，跳过",
                spec.name,
                spec.root_filter
            );
            continue;
        }

        let tree_id = Uuid::new_v4();
        insert_tree(&mut tx, tree_id, spec.code, spec.name, spec.kind, spec.desc).await?;
        tracing::info!(
            "🌱 [{}/{}] 创建树: {} (kind={}, code={})",
            idx + 1,
            TREE_SPECS.len(),
            spec.name,
            spec.kind,
            spec.code
        );

        let mut spec_count: usize = 0;
        for (root_idx, root) in roots.iter().enumerate() {
            insert_node_recursive(
                &mut tx,
                root,
                tree_id,
                None,            // parent_id = NULL（根节点）
                0i16,            // depth = 0
                String::new(),   // parent_path = ""（根节点 path 就是自身段）
                root_idx as i32, // sort_order = 根数组索引
                &mut spec_count,
            )
            .await?;
        }
        tracing::info!("   → {} 节点插入完成: {} 个", spec.name, spec_count);
        total_count += spec_count;
    }

    // ─── Step 3: 提交事务 ────────────────────────────────────────────────
    tracing::info!("✅ 所有数据插入成功，正在提交事务...");
    tx.commit().await?;
    tracing::info!("🎉 数据初始化完成！");
    tracing::info!("   ─────────────────────────────────────────");
    tracing::info!("   导入树数量: {}", TREE_SPECS.len());
    tracing::info!("   总节点数:   {}", total_count);
    tracing::info!("   ─────────────────────────────────────────");

    Ok(())
}

// ===========================================================================
// 辅助函数
// ===========================================================================

/// 定位 JSON 文件：依次尝试 CWD → CARGO_MANIFEST_DIR
fn locate_json(rel_path: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // 1. 当前工作目录
    let p = PathBuf::from(rel_path);
    if p.exists() {
        return Ok(p);
    }
    // 2. CARGO_MANIFEST_DIR（cargo run 时由 build.rs 注入）
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p2 = PathBuf::from(manifest).join(rel_path);
        if p2.exists() {
            return Ok(p2);
        }
    }
    Err(format!("JSON 文件未找到: {}（请从项目根目录运行）", rel_path).into())
}

/// 插入一棵 knowledge_trees 记录
///
/// kind 接受 'knowledge' | 'ability' | 'chapter'，SQL 端强转为 enum
async fn insert_tree(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    id: Uuid,
    code: &str,
    name: &str,
    kind: &str,
    description: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO knowledge_trees (id, code, name, kind, space_id, description, is_active)
        VALUES ($1, $2, $3, $4::text::knowledge_tree_kind, NULL, $5, TRUE)
        "#,
    )
    .bind(id)
    .bind(code)
    .bind(name)
    .bind(kind)
    .bind(description)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 递归插入节点（深度优先）
///
/// # 参数
/// - `tx`: 全局事务句柄
/// - `node`: JSON 节点（text + children）
/// - `tree_id`: 所属知识树 ID
/// - `parent_id`: 父节点 UUID（根节点为 None）
/// - `depth`: 当前节点深度（根节点为 0）
/// - `parent_path`: 父节点的 LTREE path 文本（根节点为空字符串）
/// - `sort_order`: 同级排序索引（使用 children 数组索引）
/// - `counter`: 累计节点数（用于日志统计）
///
/// # LTREE path 计算规则
/// - path 段 = `id.to_string().replace('-', "_")`（ltree 不允许 '-' 字符）
/// - 根节点 path = path 段
/// - 子节点 path = `parent_path.path段`
///
/// # code 字段
/// - 复用项目惯例：`format!("n{}", &id.to_string()[..8])`
/// - code 仅作节点独立标识，不含父级路径段
fn insert_node_recursive<'a>(
    tx: &'a mut sqlx::Transaction<'_, sqlx::Postgres>,
    node: &'a JsonNode,
    tree_id: Uuid,
    parent_id: Option<Uuid>,
    depth: i16,
    parent_path: String,
    sort_order: i32,
    counter: &'a mut usize,
) -> Pin<Box<dyn std::future::Future<Output = Result<(), sqlx::Error>> + 'a>> {
    Box::pin(async move {
        let id = Uuid::new_v4();

    // LTREE path 段：REPLACE(uuid, '-', '_')
    // ltree 标签只允许字母、数字、下划线，必须把 uuid 的 '-' 换成 '_'
    let path_segment = id.to_string().replace('-', "_");

    // 拼接完整 path：根节点 path = segment；子节点 path = parent_path.segment
    let path = if parent_path.is_empty() {
        path_segment.clone()
    } else {
        format!("{}.{}", parent_path, path_segment)
    };

    // code：项目惯例 n + id 前 8 位
    let code = format!("n{}", &id.to_string()[..8]);

    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO knowledge_nodes
          (id, tree_id, parent_id, code, path, depth, name, aliases,
           description, sort_order, question_count, is_active, created_at, updated_at)
        VALUES ($1, $2, $3, $4, text2ltree($5), $6, $7, '[]'::jsonb,
                NULL, $8, 0, TRUE, $9, $9)
        "#,
    )
    .bind(id)
    .bind(tree_id)
    .bind(parent_id)
    .bind(&code)
    .bind(&path)
    .bind(depth)
    .bind(&node.text)
    .bind(sort_order)
    .bind(now)
    .execute(&mut **tx)
    .await?;

    *counter += 1;

    // 进度日志：每 500 个节点打印一次
    if *counter % 500 == 0 {
        tracing::info!("   ... 已插入 {} 个节点", *counter);
    }

    // 递归插入子节点（用数组索引作为 sort_order，确保与 JSON 顺序一致）
    for (child_idx, child) in node.children.iter().enumerate() {
        // 提前 clone path，避免借用问题（递归调用会跨越多次 await）
        let child_parent_path = path.clone();
        insert_node_recursive(
            tx,
            child,
            tree_id,
            Some(id),
            depth + 1,
            child_parent_path,
            child_idx as i32,
            counter,
        )
            .await?;
    }

        Ok(())
    })
}
