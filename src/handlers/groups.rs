use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde_json::json;
use uuid::Uuid;

use crate::models::group::{
    AddMemberRequest, CreateGroupRequest, Group, GroupDetail, GroupMemberInfo, GroupSummary,
    SetLeaderRequest, UpdateGroupRequest,
};
use crate::AppState;

// ---------------------------------------------------------------------------
// 教研组 CRUD
// ---------------------------------------------------------------------------

/// GET /api/v1/groups — 列表（含成员数）
pub async fn list_groups(
    State(state): State<AppState>,
) -> Result<Json<Vec<GroupSummary>>, (StatusCode, Json<serde_json::Value>)> {
    let groups = sqlx::query_as::<_, GroupSummary>(
        r#"
        SELECT g.id, g.name, g.description, g.created_at,
               COUNT(gm.user_id) AS member_count
        FROM groups g
        LEFT JOIN group_members gm ON gm.group_id = g.id
        GROUP BY g.id, g.name, g.description, g.created_at
        ORDER BY g.created_at DESC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询教研组失败: {}", e)})),
        )
    })?;

    Ok(Json(groups))
}

/// POST /api/v1/groups — 创建
pub async fn create_group(
    State(state): State<AppState>,
    Json(req): Json<CreateGroupRequest>,
) -> Result<(StatusCode, Json<Group>), (StatusCode, Json<serde_json::Value>)> {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO groups (id, name, description, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5)
        "#,
    )
    .bind(id)
    .bind(&req.name)
    .bind(&req.description)
    .bind(now)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建教研组失败: {}", e)})),
        )
    })?;

    let group = sqlx::query_as::<_, Group>("SELECT * FROM groups WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询教研组失败: {}", e)})),
            )
        })?;

    Ok((StatusCode::CREATED, Json(group)))
}

/// GET /api/v1/groups/:id — 详情（含成员列表）
pub async fn get_group(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<GroupDetail>, (StatusCode, Json<serde_json::Value>)> {
    let group = sqlx::query_as::<_, Group>("SELECT * FROM groups WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询教研组失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "教研组不存在"}))))?;

    let members = sqlx::query_as::<_, GroupMemberInfo>(
        r#"
        SELECT gm.user_id, u.username, u.display_name, gm.is_leader, gm.joined_at
        FROM group_members gm
        JOIN users u ON u.id = gm.user_id
        WHERE gm.group_id = $1
        ORDER BY gm.is_leader DESC, gm.joined_at
        "#,
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员失败: {}", e)})),
        )
    })?;

    Ok(Json(GroupDetail {
        id: group.id,
        name: group.name,
        description: group.description,
        members,
        created_at: group.created_at,
    }))
}

/// PUT /api/v1/groups/:id — 更新
pub async fn update_group(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateGroupRequest>,
) -> Result<Json<Group>, (StatusCode, Json<serde_json::Value>)> {
    let existing = sqlx::query_as::<_, Group>("SELECT * FROM groups WHERE id = $1")
        .bind(id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询教研组失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "教研组不存在"}))))?;

    let new_name = req.name.unwrap_or(existing.name);
    let new_desc = req.description.or(existing.description);

    sqlx::query(
        "UPDATE groups SET name = $1, description = $2, updated_at = $3 WHERE id = $4",
    )
    .bind(&new_name)
    .bind(&new_desc)
    .bind(chrono::Utc::now())
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新教研组失败: {}", e)})),
        )
    })?;

    let updated = sqlx::query_as::<_, Group>("SELECT * FROM groups WHERE id = $1")
        .bind(id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询教研组失败: {}", e)})),
            )
        })?;

    Ok(Json(updated))
}

/// DELETE /api/v1/groups/:id — 删除
pub async fn delete_group(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let result = sqlx::query("DELETE FROM groups WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("删除教研组失败: {}", e)})),
            )
        })?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "教研组不存在"}))));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// 成员管理
// ---------------------------------------------------------------------------

/// POST /api/v1/groups/:id/members — 添加成员
pub async fn add_member(
    State(state): State<AppState>,
    Path(group_id): Path<Uuid>,
    Json(req): Json<AddMemberRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO group_members (group_id, user_id, is_leader, joined_at)
        VALUES ($1, $2, $3, $4)
        ON CONFLICT (group_id, user_id) DO UPDATE SET is_leader = $3
        "#,
    )
    .bind(group_id)
    .bind(req.user_id)
    .bind(req.is_leader.unwrap_or(false))
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("添加成员失败: {}", e)})),
        )
    })?;

    Ok((
        StatusCode::CREATED,
        Json(json!({"message": "成员添加成功"})),
    ))
}

/// DELETE /api/v1/groups/:id/members/:user_id — 移除成员
pub async fn remove_member(
    State(state): State<AppState>,
    Path((group_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let result = sqlx::query(
        "DELETE FROM group_members WHERE group_id = $1 AND user_id = $2",
    )
    .bind(group_id)
    .bind(user_id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("移除成员失败: {}", e)})),
        )
    })?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "成员不存在"}))));
    }

    Ok(StatusCode::NO_CONTENT)
}

/// PUT /api/v1/groups/:id/members/:user_id — 设置组长
pub async fn set_leader(
    State(state): State<AppState>,
    Path((group_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<SetLeaderRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let result = sqlx::query(
        "UPDATE group_members SET is_leader = $1 WHERE group_id = $2 AND user_id = $3",
    )
    .bind(req.is_leader)
    .bind(group_id)
    .bind(user_id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("设置组长失败: {}", e)})),
        )
    })?;

    if result.rows_affected() == 0 {
        return Err((StatusCode::NOT_FOUND, Json(json!({"error": "成员不存在"}))));
    }

    Ok(Json(json!({"message": "组长设置成功"})))
}
