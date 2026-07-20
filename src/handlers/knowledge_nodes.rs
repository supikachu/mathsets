//! 知识点节点 CRUD + LTREE 子树查询（B3 新增）
//!
//! 核心设计：
//! - `path` 字段在 SQL 中是 LTREE 类型，Rust 端用 String 接收
//!   读取时用 `path::text AS path`，写入时用 `text2ltree($x)`
//! - 子树查询使用 LTREE 的 `<@`（被包含）和 `@>`（包含）操作符，命中 GiST 索引
//! - 移动节点时，用 `nlevel()` 和 `subltree()` 函数批量重算所有子孙的 path 和 depth

use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::models::question::{
    CreateKnowledgeNodeRequest, KnowledgeNode, KnowledgeNodeTreeNode, MoveKnowledgeNodeRequest,
    UpdateKnowledgeNodeRequest,
};
use crate::AppState;

// ===========================================================================
// 查询参数
// ===========================================================================

/// 节点查询参数
#[derive(Debug, Deserialize)]
pub struct NodeQuery {
    /// 按父节点过滤（NULL = 仅根节点）
    pub parent_id: Option<Uuid>,
    /// 是否包含所有子孙节点（LTREE 子树查询）
    #[serde(default)]
    pub include_descendants: bool,
}

/// 子树查询参数
#[derive(Debug, Deserialize)]
pub struct SubtreeQuery {
    /// 是否包含自身
    #[serde(default = "default_true")]
    pub include_self: bool,
    /// 最大深度限制（相对根节点，0=仅自身，1=直接子节点...）
    pub max_depth: Option<i16>,
}

fn default_true() -> bool {
    true
}

// ===========================================================================
// 树形构建（Rust 端递归，替代旧 build_tree）
// ===========================================================================

/// 在 Rust 端从扁平节点列表构建树
///
/// 性能考虑：单次 SELECT 全部节点后内存递归，避免 N+1 查询。
/// 节点数 >5000 时建议改用前端扁平渲染。
pub fn build_node_tree(nodes: &[KnowledgeNode]) -> Vec<KnowledgeNodeTreeNode> {
    build_subtree(nodes, None)
}

fn build_subtree(nodes: &[KnowledgeNode], parent_id: Option<Uuid>) -> Vec<KnowledgeNodeTreeNode> {
    let mut children: Vec<KnowledgeNodeTreeNode> = nodes
        .iter()
        .filter(|n| n.parent_id == parent_id)
        .map(|n| {
            let mut node = KnowledgeNodeTreeNode::from(n.clone());
            node.children = build_subtree(nodes, Some(n.id));
            node
        })
        .collect();
    children.sort_by_key(|n| n.sort_order);
    children
}

// ===========================================================================
// 共享 SQL 片段：path::text 转换
// ===========================================================================

/// 将 SQL 中的 LTREE path 列读取为 String 的标准 SELECT 模板
const NODE_SELECT_FIELDS: &str = r#"
    id, tree_id, parent_id, code,
    path::text AS path,
    depth, name, aliases, description,
    sort_order, question_count, is_active,
    created_at, updated_at
"#;

// ===========================================================================
// API Handlers
// ===========================================================================

/// GET /api/v1/knowledge-trees/{tree_id}/nodes — 获取指定树的全部节点（扁平结构）
///
/// 前端可在内存中构建树（build_node_tree）。
/// 性能：单次 SELECT，避免 N+1 查询。
pub async fn list_nodes_by_tree(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<KnowledgeNode>>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let nodes = sqlx::query_as::<_, KnowledgeNode>(&format!(
        r#"
        SELECT {NODE_SELECT_FIELDS}
        FROM knowledge_nodes
        WHERE tree_id = $1 AND is_active = TRUE
        ORDER BY sort_order, name
        "#,
    ))
    .bind(tree_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询知识点节点失败: {e}")})),
        )
    })?;

    Ok(Json(nodes))
}

/// GET /api/v1/knowledge-trees/{tree_id}/nodes/tree — 获取指定树的节点（树形结构）
///
/// 直接返回带 children 的树形 JSON，前端无需二次构建。
pub async fn get_node_tree(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(tree_id): Path<Uuid>,
) -> Result<Json<Vec<KnowledgeNodeTreeNode>>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let nodes = sqlx::query_as::<_, KnowledgeNode>(&format!(
        r#"
        SELECT {NODE_SELECT_FIELDS}
        FROM knowledge_nodes
        WHERE tree_id = $1 AND is_active = TRUE
        ORDER BY sort_order, name
        "#,
    ))
    .bind(tree_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询知识点树失败: {e}")})),
        )
    })?;

    let tree = build_node_tree(&nodes);
    Ok(Json(tree))
}

/// GET /api/v1/knowledge-nodes/{id} — 获取单个节点详情
pub async fn get_node(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<KnowledgeNode>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let node = sqlx::query_as::<_, KnowledgeNode>(&format!(
        r#"
        SELECT {NODE_SELECT_FIELDS}
        FROM knowledge_nodes
        WHERE id = $1
        "#,
    ))
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询知识点节点失败: {e}")})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "知识点节点不存在"})),
        )
    })?;

    Ok(Json(node))
}

/// GET /api/v1/knowledge-nodes/{id}/descendants — LTREE 子树查询
///
/// 利用 LTREE `<@` 操作符命中 GiST 索引，单次查询获取所有子孙节点。
/// 这是支持"按知识点子树过滤题目"的核心 API。
pub async fn get_descendants(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Query(query): Query<SubtreeQuery>,
) -> Result<Json<Vec<KnowledgeNode>>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    // 先取目标节点 path 和 depth
    let root: Option<(String, i16)> = sqlx::query_as(
        "SELECT path::text AS path, depth FROM knowledge_nodes WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询根节点失败: {e}")})),
        )
    })?;

    let (root_path, root_depth) = root.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "知识点节点不存在"})),
        )
    })?;

    // 构造子树查询条件：
    // - include_self=false: path <@ root_path AND path != root_path
    // - include_self=true:  path <@ root_path
    // - max_depth: depth - root_depth <= max_depth
    let nodes = sqlx::query_as::<_, KnowledgeNode>(&format!(
        r#"
        SELECT {NODE_SELECT_FIELDS}
        FROM knowledge_nodes
        WHERE path <@ text2ltree($1)
          AND ($2::bool OR path::text <> $1)
          AND ($3::int IS NULL OR depth - $4 <= $3)
          AND is_active = TRUE
        ORDER BY path
        "#,
    ))
    .bind(&root_path)
    .bind(query.include_self)
    .bind(query.max_depth.map(|d| d as i32))
    .bind(root_depth)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("子树查询失败: {e}")})),
        )
    })?;

    Ok(Json(nodes))
}

/// POST /api/v1/knowledge-nodes — 创建节点
///
/// 自动计算 path 和 depth：
/// - 根节点：path = code, depth = 0
/// - 子节点：path = parent.path || '.' || code, depth = parent.depth + 1
pub async fn create_node(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Json(req): Json<CreateKnowledgeNodeRequest>,
) -> Result<(StatusCode, Json<KnowledgeNode>), (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let id = Uuid::new_v4();
    let now = chrono::Utc::now();

    // 自动生成 code（若未提供，用 id 前 8 字符，前缀 n）
    // code 仅作节点独立标识，不含父级路径段（吸收 B1 修正#1）
    let code = req.code.unwrap_or_else(|| format!("n{}", &id.to_string()[..8]));

    // 计算 path 和 depth
    let (path, depth) = if let Some(parent_id) = req.parent_id {
        let parent: (String, i16) = sqlx::query_as(
            "SELECT path::text AS path, depth FROM knowledge_nodes WHERE id = $1",
        )
        .bind(parent_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询父节点失败: {e}")})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "父节点不存在"})),
            )
        })?;

        (format!("{}.{}", parent.0, code), parent.1 + 1)
    } else {
        (code.clone(), 0)
    };

    let aliases = req.aliases.unwrap_or_else(|| json!([]));

    sqlx::query(
        r#"
        INSERT INTO knowledge_nodes
          (id, tree_id, parent_id, code, path, depth, name, aliases, description, sort_order, question_count, is_active, created_at, updated_at)
        VALUES ($1, $2, $3, $4, text2ltree($5), $6, $7, $8, $9, $10, 0, TRUE, $11, $11)
        "#,
    )
    .bind(id)
    .bind(req.tree_id)
    .bind(req.parent_id)
    .bind(&code)
    .bind(&path)
    .bind(depth)
    .bind(&req.name)
    .bind(&aliases)
    .bind(&req.description)
    .bind(req.sort_order.unwrap_or(0))
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        let msg = format!("{e}");
        let status = if msg.contains("duplicate") || msg.contains("unique") {
            StatusCode::CONFLICT
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, Json(json!({"error": format!("创建知识点节点失败: {msg}")})))
    })?;

    // 读取创建后的节点
    let node = sqlx::query_as::<_, KnowledgeNode>(&format!(
        r#"
        SELECT {NODE_SELECT_FIELDS}
        FROM knowledge_nodes
        WHERE id = $1
        "#,
    ))
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("读取新节点失败: {e}")})),
        )
    })?;

    Ok((StatusCode::CREATED, Json(node)))
}

/// PUT /api/v1/knowledge-nodes/{id} — 更新节点元数据（不含 parent_id，移动用 move_node）
pub async fn update_node(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateKnowledgeNodeRequest>,
) -> Result<Json<KnowledgeNode>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    let node = sqlx::query_as::<_, KnowledgeNode>(&format!(
        r#"
        UPDATE knowledge_nodes
        SET name        = COALESCE($1, name),
            code        = COALESCE($2, code),
            aliases     = COALESCE($3, aliases),
            description = COALESCE($4, description),
            sort_order  = COALESCE($5, sort_order),
            is_active   = COALESCE($6, is_active),
            updated_at  = NOW()
        WHERE id = $7
        RETURNING {NODE_SELECT_FIELDS}
        "#,
    ))
    .bind(req.name)
    .bind(req.code)
    .bind(req.aliases)
    .bind(req.description)
    .bind(req.sort_order)
    .bind(req.is_active)
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新知识点节点失败: {e}")})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "知识点节点不存在"})),
        )
    })?;

    Ok(Json(node))
}

/// POST /api/v1/knowledge-nodes/{id}/move — 移动节点（改 parent_id，重算所有子孙的 path 与 depth）
///
/// 利用 LTREE 的 `nlevel()` 和 `subltree()` 函数批量重算路径：
/// - `nlevel(path)` = path 中的节点数（深度+1）
/// - `subltree(path, start, end)` = path 的 [start, end) 子串
///
/// 核心公式：new_path = new_parent_path || subltree(old_path, nlevel(old_root) - 1, nlevel(target_path))
pub async fn move_node(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<MoveKnowledgeNodeRequest>,
) -> Result<Json<Vec<KnowledgeNode>>, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    // 防御：不能把自己移到自己的子树下（会形成环）
    if let Some(new_pid) = req.new_parent_id {
        if new_pid == id {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "不能将节点移动到自身下"})),
            ));
        }

        // 检查目标父节点是否是当前节点的子孙
        let is_descendant: bool = sqlx::query_scalar(
            r#"
            SELECT EXISTS(
              SELECT 1 FROM knowledge_nodes
              WHERE id = $1 AND path <@ (SELECT path FROM knowledge_nodes WHERE id = $2)
            )
            "#,
        )
        .bind(new_pid)
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("环检查失败: {e}")})),
            )
        })?;

        if is_descendant {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "不能将节点移动到其子孙下（会形成环）"})),
            ));
        }
    }

    // 读取要移动节点的当前 path 和 depth
    let old_root: (String, i16) = sqlx::query_as(
        "SELECT path::text AS path, depth FROM knowledge_nodes WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询源节点失败: {e}")})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "知识点节点不存在"})),
        )
    })?;

    // 读取新父节点的 path 和 depth（若 new_parent_id 为 None，则为根）
    let (new_parent_path, new_parent_depth): (Option<String>, i16) = if let Some(npid) =
        req.new_parent_id
    {
        let np: (String, i16) = sqlx::query_as(
            "SELECT path::text AS path, depth FROM knowledge_nodes WHERE id = $1",
        )
        .bind(npid)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询目标父节点失败: {e}")})),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "目标父节点不存在"})),
            )
        })?;
        (Some(np.0), np.1)
    } else {
        (None, -1) // 根节点 depth = 0，新 depth = 0 + 1 - 1 = 0
    };

    let depth_delta = new_parent_depth + 1 - old_root.1;

    // 批量更新该节点及其所有子孙的 path 和 depth
    // 核心公式：new_path = new_parent_path || subltree(old_path, nlevel(old_root) - 1, nlevel(old_path))
    // subltree 提取从 old_root 自身开始的部分（去除祖先前缀），再拼到新父路径下
    if let Some(np_path) = new_parent_path {
        sqlx::query(
            r#"
            WITH old_root AS (
              SELECT path FROM knowledge_nodes WHERE id = $1
            )
            UPDATE knowledge_nodes
            SET 
              path = text2ltree($2) || subltree(path, nlevel((SELECT path FROM old_root)) - 1, nlevel(path)),
              depth = depth + $3,
              updated_at = NOW()
            WHERE path <@ (SELECT path FROM old_root)
            "#,
        )
        .bind(id)
        .bind(&np_path)
        .bind(depth_delta)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("重算 path 失败: {e}")})),
            )
        })?;
    } else {
        // 移动到根：去掉祖先前缀，只保留 old_root 自身开始的部分
        sqlx::query(
            r#"
            WITH old_root AS (
              SELECT path FROM knowledge_nodes WHERE id = $1
            )
            UPDATE knowledge_nodes
            SET 
              path = subltree(path, nlevel((SELECT path FROM old_root)) - 1, nlevel(path)),
              depth = depth + $2,
              updated_at = NOW()
            WHERE path <@ (SELECT path FROM old_root)
            "#,
        )
        .bind(id)
        .bind(depth_delta)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("重算 path 失败: {e}")})),
            )
        })?;
    }

    // 更新 parent_id（在 path 重算之后单独 UPDATE，避免 CTE 引用问题）
    sqlx::query("UPDATE knowledge_nodes SET parent_id = $1, updated_at = NOW() WHERE id = $2")
        .bind(req.new_parent_id)
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("更新 parent_id 失败: {e}")})),
            )
        })?;

    // 读取移动后的整棵子树（含自身）返回
    let nodes = sqlx::query_as::<_, KnowledgeNode>(&format!(
        r#"
        SELECT {NODE_SELECT_FIELDS}
        FROM knowledge_nodes
        WHERE path <@ (SELECT path FROM knowledge_nodes WHERE id = $1)
        ORDER BY path
        "#,
    ))
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("读取移动后子树失败: {e}")})),
        )
    })?;

    Ok(Json(nodes))
}

/// DELETE /api/v1/knowledge-nodes/{id} — 删除节点
///
/// 约束：
/// 1. 有子节点时拒绝删除（避免误删整棵子树）
/// 2. 有题目关联时拒绝删除
pub async fn delete_node(
    State(state): State<AppState>,
    Extension(auth_user): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let _ = auth_user;

    // 检查是否有子节点
    let child_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM knowledge_nodes WHERE parent_id = $1",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询子节点失败: {e}")})),
        )
    })?;

    if child_count > 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "该节点下有子节点，请先删除子节点或使用移动功能"})),
        ));
    }

    // 检查是否有题目关联
    let ref_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM question_knowledge_nodes WHERE node_id = $1",
    )
    .bind(id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询题目关联失败: {e}")})),
        )
    })?;

    if ref_count > 0 {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": format!("该节点有 {} 个题目关联，请先解除关联", ref_count)})),
        ));
    }

    let result = sqlx::query("DELETE FROM knowledge_nodes WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("删除知识点节点失败: {e}")})),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "知识点节点不存在"})),
        ));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ===========================================================================
// 内部辅助函数（供 questions.rs 等模块复用）
// ===========================================================================

/// 获取指定节点列表的子树 path（用于 questions.rs 的 include_descendants 查询）
///
/// 输入一组 node_id，返回所有这些节点及其子孙的 path 数组。
/// 用于 `WHERE qkn.node_id IN (SELECT id FROM knowledge_nodes WHERE path <@ ANY($1::ltree[]))`
pub async fn fetch_subtree_paths(
    pool: &sqlx::PgPool,
    node_ids: &[Uuid],
) -> Result<Vec<String>, (StatusCode, Json<serde_json::Value>)> {
    if node_ids.is_empty() {
        return Ok(vec![]);
    }

    let paths: Vec<String> = sqlx::query_scalar(
        r#"
        SELECT DISTINCT ancestor.path::text
        FROM knowledge_nodes selected
        JOIN knowledge_nodes ancestor ON ancestor.path @> selected.path
        WHERE selected.id = ANY($1)
        "#,
    )
    .bind(node_ids)
    .fetch_all(pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("获取子树 path 失败: {e}")})),
        )
    })?;

    Ok(paths)
}
