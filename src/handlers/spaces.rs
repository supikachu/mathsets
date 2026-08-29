use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::{
    can_access_space, ensure_personal_space, ensure_public_space, get_space, is_admin_user,
    is_space_member,
};
use crate::handlers::notifications::send_notification;
use crate::handlers::questions::{build_detail, db_err, save_version};
use crate::models::notification::CreateNotification;
use crate::models::question::{Question, QuestionStatus, TransferQuestionRequest};
use crate::models::space::{
    AddSpaceMemberRequest, CreateTeamSpaceRequest, Space, SpaceDetail, SpaceKind, SpaceMemberInfo,
    SpaceRole, SpaceSummary, UpdateSpaceMemberRequest, UpdateSpaceRequest,
};
use crate::AppState;

/// GET /api/v1/spaces — 我可访问的空间（个人 + 我加入的团队 + 公共）
pub async fn list_spaces(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<SpaceSummary>>, (StatusCode, Json<serde_json::Value>)> {
    // 阶段 0：确保公共空间存在
    if let Err(e) = ensure_public_space(&state.pool).await {
        tracing::error!("list_spaces ensure_public_space 失败: {:?}", e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("初始化公共空间失败: {}", e)})),
        ));
    }

    // 阶段 1：查询用户可访问的空间列表
    let rows = sqlx::query_as::<_, SpaceSummary>(
        r#"
        SELECT s.id, s.kind, s.name, s.owner_user_id,
               CASE
                 WHEN s.kind = 'team' THEN (
                   SELECT COUNT(*) FROM space_members sm WHERE sm.space_id = s.id
                 )
                 WHEN s.kind = 'personal' THEN 1
                 ELSE NULL
               END AS member_count,
               CASE
                 WHEN s.kind = 'personal' AND s.owner_user_id = $1 THEN 'owner'
                 WHEN s.kind = 'public' THEN 'viewer'
                 ELSE (
                   SELECT sm.role FROM space_members sm
                   WHERE sm.space_id = s.id AND sm.user_id = $1
                 )
               END::text AS my_role,
               s.created_at
        FROM spaces s
        WHERE s.kind = 'public'
           OR (s.kind = 'personal' AND s.owner_user_id = $1)
           OR (s.kind = 'team' AND EXISTS (
                 SELECT 1 FROM space_members sm
                 WHERE sm.space_id = s.id AND sm.user_id = $1
               ))
           OR ($2 AND s.kind = 'team')
        ORDER BY
          CASE s.kind
            WHEN 'personal' THEN 0
            WHEN 'team' THEN 1
            WHEN 'public' THEN 2
          END,
          s.created_at ASC
        "#,
    )
    .bind(auth.id)
    .bind(is_admin_user(&auth))
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        // 【最佳实践】用 Debug format 输出完整 sqlx::Error 调用栈，
        // 包含 ColumnNotFound / TypeMismatch / PoolDisconnected 等底层信息，
        // 防止真正的底层 SQL 错误被 Display format 吞噬
        tracing::error!(
            "list_spaces 查询失败 (user_id={}, role={}): {:?}",
            auth.id,
            auth.role,
            e
        );
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询空间失败: {}", e)})),
        )
    })?;

    Ok(Json(rows))
}

/// POST /api/v1/spaces — 创建团队空间（创建者为 owner）
pub async fn create_team_space(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<CreateTeamSpaceRequest>,
) -> Result<(StatusCode, Json<Space>), (StatusCode, Json<serde_json::Value>)> {
    if req.name.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "空间名称不能为空"})),
        ));
    }

    let id = Uuid::new_v4();
    let now = chrono::Utc::now();
    let settings = serde_json::json!({
        "allow_creator_self_review": true,
        "require_review_duty": false
    });

    let mut tx = state.pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("开启事务失败: {}", e)})),
        )
    })?;

    sqlx::query(
        r#"
        INSERT INTO spaces (id, kind, name, owner_user_id, settings, created_at, updated_at)
        VALUES ($1, 'team', $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(req.name.trim())
    .bind(auth.id)
    .bind(&settings)
    .bind(now)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建空间失败: {}", e)})),
        )
    })?;

    sqlx::query(
        r#"
        INSERT INTO space_members (space_id, user_id, role, duties, joined_at)
        VALUES ($1, $2, $3, '{}', $4)
        "#,
    )
    .bind(id)
    .bind(auth.id)
    .bind(SpaceRole::Owner)
    .bind(now)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("添加成员失败: {}", e)})),
        )
    })?;

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("提交事务失败: {}", e)})),
        )
    })?;

    let space = get_space(&state.pool, id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "空间创建后丢失"}))))?;

    Ok((StatusCode::CREATED, Json(space)))
}

/// GET /api/v1/spaces/:id
pub async fn get_space_detail(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<Json<SpaceDetail>, (StatusCode, Json<serde_json::Value>)> {
    let space = get_space(&state.pool, id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if !can_access_space(&state.pool, &auth, &space)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("权限检查失败: {}", e)})),
            )
        })?
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权访问该空间"}))));
    }

    let members = if space.kind == SpaceKind::Team {
        sqlx::query_as::<_, SpaceMemberInfo>(
            r#"
            SELECT sm.user_id, u.username, u.display_name, sm.role::text AS role, sm.duties, sm.joined_at
            FROM space_members sm
            JOIN users u ON u.id = sm.user_id
            WHERE sm.space_id = $1
            ORDER BY CASE sm.role WHEN 'owner' THEN 0 ELSE 1 END, sm.joined_at
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
        })?
    } else {
        vec![]
    };

    Ok(Json(SpaceDetail {
        id: space.id,
        kind: space.kind,
        name: space.name,
        owner_user_id: space.owner_user_id,
        settings: space.settings,
        members,
        created_at: space.created_at,
    }))
}

/// PUT /api/v1/spaces/:id
pub async fn update_space(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateSpaceRequest>,
) -> Result<Json<Space>, (StatusCode, Json<serde_json::Value>)> {
    let space = get_space(&state.pool, id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if space.kind == SpaceKind::Public && !is_admin_user(&auth) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权修改公共空间"}))));
    }

    let is_owner = match space.kind {
        SpaceKind::Personal => space.owner_user_id == Some(auth.id),
        SpaceKind::Team => {
            let role: Option<String> = sqlx::query_scalar(
                "SELECT role::text FROM space_members WHERE space_id = $1 AND user_id = $2",
            )
            .bind(id)
            .bind(auth.id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("查询成员失败: {}", e)})),
                )
            })?;
            role.as_deref() == Some("owner")
        }
        SpaceKind::Public => is_admin_user(&auth),
    };

    if !is_owner && !is_admin_user(&auth) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "仅空间所有者可修改"}))));
    }

    let new_name = req.name.unwrap_or(space.name);
    let new_settings = if let Some(s) = req.settings {
        serde_json::to_value(s).unwrap_or(space.settings)
    } else {
        space.settings
    };

    sqlx::query(
        "UPDATE spaces SET name = $1, settings = $2, updated_at = $3 WHERE id = $4",
    )
    .bind(&new_name)
    .bind(&new_settings)
    .bind(chrono::Utc::now())
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新空间失败: {}", e)})),
        )
    })?;

    let updated = get_space(&state.pool, id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    Ok(Json(updated))
}

/// DELETE /api/v1/spaces/:id — 仅团队空间，owner/Admin
pub async fn delete_space(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let space = get_space(&state.pool, id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if space.kind != SpaceKind::Team {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只能删除团队空间"})),
        ));
    }

    let is_owner = sqlx::query_scalar::<_, String>(
        "SELECT role::text FROM space_members WHERE space_id = $1 AND user_id = $2",
    )
    .bind(id)
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员失败: {}", e)})),
        )
    })?
    .as_deref()
        == Some("owner");

    if !is_owner && !is_admin_user(&auth) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权删除该空间"}))));
    }

    // ── 删除前快照：原成员列表 + 空间名（用于事务后通知） ──
    let member_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM space_members WHERE space_id = $1",
    )
    .bind(id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员列表失败: {}", e)})),
        )
    })?;

    let space_name = space.name.clone();

    // ── 事务前：确保 Owner 个人空间存在 ──
    let personal_space_id = ensure_personal_space(&state.pool, auth.id, &auth.username)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("获取个人空间失败: {}", e)})),
            )
        })?;

    // ── 事务：批量转移题目 → 清理成员 → 删除空间 ──
    let mut tx = state.pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("开启事务失败: {}", e)})),
        )
    })?;

    // 1. 将所有题目资产转移到 Owner 的个人空间
    sqlx::query("UPDATE questions SET space_id = $1 WHERE space_id = $2")
        .bind(personal_space_id)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("转移题目资产失败: {}", e)})),
            )
        })?;

    // 2. 清理成员映射表
    sqlx::query("DELETE FROM space_members WHERE space_id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("清理成员失败: {}", e)})),
            )
        })?;

    // 3. 物理删除空间
    sqlx::query("DELETE FROM spaces WHERE id = $1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("删除空间失败: {}", e)})),
            )
        })?;

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("提交事务失败: {}", e)})),
        )
    })?;

    // ── 事务后：向所有原成员发送系统通知（SSE 广播） ──
    for member_id in &member_ids {
        if let Err(e) = send_notification(
            &state.pool,
            &state.notify_tx,
            CreateNotification {
                user_id: *member_id,
                kind: "system".into(),
                title: "团队空间已解散".into(),
                body: Some(format!(
                    "您所在的团队空间「{}」已被解散，相关题目资产已由管理员接收",
                    space_name
                )),
                resource_type: Some("space".into()),
                resource_id: None,
            },
        )
        .await
        {
            tracing::warn!("解散空间通知发送失败, member_id={}, err={}", member_id, e);
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/v1/spaces/:id/members
pub async fn add_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(space_id): Path<Uuid>,
    Json(req): Json<AddSpaceMemberRequest>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    let space = get_space(&state.pool, space_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if space.kind != SpaceKind::Team {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只能向团队空间添加成员"})),
        ));
    }

    let my_role: Option<String> = sqlx::query_scalar(
        "SELECT role::text FROM space_members WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员失败: {}", e)})),
        )
    })?;

    if my_role.as_deref() != Some("owner") && !is_admin_user(&auth) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "仅空间拥有者可进行成员管理"}))));
    }

    let role = req.role.unwrap_or_else(|| "member".into());
    if !matches!(role.as_str(), "owner" | "member" | "viewer") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "role 必须是 owner、member 或 viewer"})),
        ));
    }
    let duties = req.duties.unwrap_or_default();
    let now = chrono::Utc::now();

    // 按 username 解析 user_id（避免前端依赖 /admin/users 接口）
    let user_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM users WHERE username = $1 AND is_active = true",
    )
    .bind(req.username.trim())
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询用户失败: {}", e)})),
        )
    })?
    .ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("用户 '{}' 不存在或已停用", req.username.trim())})),
        )
    })?;

    sqlx::query(
        r#"
        INSERT INTO space_members (space_id, user_id, role, duties, joined_at)
        VALUES ($1, $2, $3::space_role, $4, $5)
        ON CONFLICT (space_id, user_id) DO UPDATE SET role = $3::space_role, duties = $4
        "#,
    )
    .bind(space_id)
    .bind(user_id)
    .bind(&role)
    .bind(&duties)
    .bind(now)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("添加成员失败: {}", e)})),
        )
    })?;

    // ── 入群通知：向新成员发送 SSE 通知 ──
    if let Err(e) = send_notification(
        &state.pool,
        &state.notify_tx,
        CreateNotification {
            user_id,
            kind: "system".into(),
            title: "加入空间通知".into(),
            body: Some(format!("您已被加入团队空间「{}」", space.name)),
            resource_type: Some("space".into()),
            resource_id: Some(space_id),
        },
    )
    .await
    {
        tracing::warn!("入群通知发送失败, user_id={}, err={}", user_id, e);
    }

    Ok((
        StatusCode::CREATED,
        Json(json!({"message": "成员添加成功"})),
    ))
}

/// PUT /api/v1/spaces/:id/members/:user_id
pub async fn update_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path((space_id, user_id)): Path<(Uuid, Uuid)>,
    Json(req): Json<UpdateSpaceMemberRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let my_role: Option<String> = sqlx::query_scalar(
        "SELECT role::text FROM space_members WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员失败: {}", e)})),
        )
    })?;

    if my_role.as_deref() != Some("owner") && !is_admin_user(&auth) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "仅空间拥有者可进行成员管理"}))));
    }

    let existing = sqlx::query_as::<_, (String, Vec<String>)>(
        "SELECT role::text, duties FROM space_members WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
    .bind(user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员失败: {}", e)})),
        )
    })?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "成员不存在"}))))?;

    // 仅当显式指定新角色时校验，避免旧数据（editor/reviewer）更新 duties 时被阻断
    if let Some(ref new_role) = req.role {
        if !matches!(new_role.as_str(), "owner" | "member" | "viewer") {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "role 必须是 owner、member 或 viewer"})),
            ));
        }
    }
    let role = req.role.unwrap_or(existing.0);
    let duties = req.duties.unwrap_or(existing.1);

    sqlx::query(
        "UPDATE space_members SET role = $1::space_role, duties = $2 WHERE space_id = $3 AND user_id = $4",
    )
    .bind(&role)
    .bind(&duties)
    .bind(space_id)
    .bind(user_id)
    .execute(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新成员失败: {}", e)})),
        )
    })?;

    Ok(Json(json!({"message": "成员已更新"})))
}

/// DELETE /api/v1/spaces/:id/members/:user_id
pub async fn remove_member(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path((space_id, user_id)): Path<(Uuid, Uuid)>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let my_role: Option<String> = sqlx::query_scalar(
        "SELECT role::text FROM space_members WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员失败: {}", e)})),
        )
    })?;

    let self_leave = user_id == auth.id;
    if !self_leave && my_role.as_deref() != Some("owner") && !is_admin_user(&auth) {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "仅空间拥有者可进行成员管理"}))));
    }

    let result = sqlx::query(
        "DELETE FROM space_members WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
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

/// PUT /api/v1/spaces/:id/transfer/:target_user_id — 转让空间所有权
///
/// 事务内完成"一升一降"：目标用户升为 Owner，当前用户降为 Member。
/// 仅当前 Owner（或系统 Admin）可发起转让；不能转让给自己。
pub async fn transfer_ownership(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path((space_id, target_user_id)): Path<(Uuid, Uuid)>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let space = get_space(&state.pool, space_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if space.kind != SpaceKind::Team {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只能转让团队空间"})),
        ));
    }

    // 校验当前用户是 Owner
    let my_role: Option<String> = sqlx::query_scalar(
        "SELECT role::text FROM space_members WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员失败: {}", e)})),
        )
    })?;

    if my_role.as_deref() != Some("owner") && !is_admin_user(&auth) {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅空间拥有者可转让权限"})),
        ));
    }

    // 不能转让给自己
    if target_user_id == auth.id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "不能转让给自己"})),
        ));
    }

    // 校验目标用户是该空间成员
    let target_role: Option<String> = sqlx::query_scalar(
        "SELECT role::text FROM space_members WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
    .bind(target_user_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询目标成员失败: {}", e)})),
        )
    })?;

    if target_role.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "目标用户不是该空间成员"})),
        ));
    }

    // 事务：目标用户升 Owner，当前用户降 Member（一升一降，保证单 Owner）
    let mut tx = state.pool.begin().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("开启事务失败: {}", e)})),
        )
    })?;

    sqlx::query(
        "UPDATE space_members SET role = 'owner'::space_role WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
    .bind(target_user_id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("升级目标用户失败: {}", e)})),
        )
    })?;

    sqlx::query(
        "UPDATE space_members SET role = 'member'::space_role WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
    .bind(auth.id)
    .execute(&mut *tx)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("降级当前用户失败: {}", e)})),
        )
    })?;

    tx.commit().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("提交事务失败: {}", e)})),
        )
    })?;

    // ── 事务后：向所有成员广播通知（SSE） ──
    let all_members: Vec<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM space_members WHERE space_id = $1",
    )
    .bind(space_id)
    .fetch_all(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员列表失败: {}", e)})),
        )
    })?;

    // 查询新 Owner 的用户名
    let new_owner_name: String = sqlx::query_scalar(
        "SELECT username FROM users WHERE id = $1",
    )
    .bind(target_user_id)
    .fetch_one(&state.pool)
    .await
    .unwrap_or_else(|_| "未知用户".into());

    let space_name = space.name.clone();
    for member_id in &all_members {
        if let Err(e) = send_notification(
            &state.pool,
            &state.notify_tx,
            CreateNotification {
                user_id: *member_id,
                kind: "system".into(),
                title: "空间拥有者变更".into(),
                body: Some(format!(
                    "团队空间「{}」的拥有者已变更为 {}",
                    space_name, new_owner_name
                )),
                resource_type: Some("space".into()),
                resource_id: Some(space_id),
            },
        )
        .await
        {
            tracing::warn!("转让通知发送失败, member_id={}, err={}", member_id, e);
        }
    }

    Ok(Json(json!({"message": "权限转让成功"})))
}

/// DELETE /api/v1/spaces/:id/leave — 成员退出空间
///
/// Owner 不能直接退出（需先转让或解散），Member/Viewer 可自行退出。
pub async fn leave_space(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(space_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let space = get_space(&state.pool, space_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if space.kind != SpaceKind::Team {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error": "只能退出团队空间"})),
        ));
    }

    let my_role: Option<String> = sqlx::query_scalar(
        "SELECT role::text FROM space_members WHERE space_id = $1 AND user_id = $2",
    )
    .bind(space_id)
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询成员失败: {}", e)})),
        )
    })?;

    if my_role.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "您不是该空间成员"})),
        ));
    }

    // Owner 不能直接退出 —— 必须先转让或解散
    if my_role.as_deref() == Some("owner") {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "拥有者只能解散空间或转让权限，不能直接退出"})),
        ));
    }

    sqlx::query("DELETE FROM space_members WHERE space_id = $1 AND user_id = $2")
        .bind(space_id)
        .bind(auth.id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("退出空间失败: {}", e)})),
            )
        })?;

    // ── 退出后：向空间 Owner 发送通知（SSE 广播） ──
    let owner_id: Option<Uuid> = sqlx::query_scalar(
        "SELECT user_id FROM space_members WHERE space_id = $1 AND role = 'owner'",
    )
    .bind(space_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("查询空间 Owner 失败: {}", e)})),
        )
    })?;

    if let Some(owner) = owner_id {
        if let Err(e) = send_notification(
            &state.pool,
            &state.notify_tx,
            CreateNotification {
                user_id: owner,
                kind: "system".into(),
                title: "成员退出空间".into(),
                body: Some(format!(
                    "成员 {} 已退出团队空间「{}」",
                    auth.username, space.name
                )),
                resource_type: Some("space".into()),
                resource_id: Some(space_id),
            },
        )
        .await
        {
            tracing::warn!("成员退出通知发送失败, owner_id={}, err={}", owner, e);
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

/// 内部：校验成员访问（供其他 handler 复用 re-export）
pub async fn require_space_member(
    pool: &sqlx::PgPool,
    auth: &AuthUser,
    space_id: Uuid,
) -> Result<Space, (StatusCode, Json<serde_json::Value>)> {
    let space = get_space(pool, space_id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("查询空间失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "空间不存在"}))))?;

    if !is_space_member(pool, space_id, auth.id)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("权限检查失败: {}", e)})),
            )
        })?
        && !is_admin_user(&auth)
    {
        return Err((StatusCode::FORBIDDEN, Json(json!({"error": "无权访问该空间"}))));
    }

    Ok(space)
}

// ---------------------------------------------------------------------------
// 跨空间克隆题目（深拷贝 + 强制 Draft + origin_question_id 链路）
// ---------------------------------------------------------------------------

/// POST /api/v1/questions/:id/clone — 跨空间克隆题目
///
/// 业务规则：
/// - 将传入 `question_id` 的源题深拷贝到 `target_space_id`
/// - 若请求体中缺省 `target_space_id`，则默认克隆到当前用户的 Personal 空间
/// - 克隆产生的新题目 `status` 强制重置为 `Draft`
/// - 准确记录 `origin_question_id` 指向源题
/// - 统计字段（paper_count/attempt_count/favorite_count）清零，accuracy_rate 置 NULL
/// - 拷贝知识点关联与标签关联（标签 use_count 同步递增）
/// - 通过事务保证原子性，tx.commit() 后才返回新题详情
pub async fn clone_question(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(question_id): Path<Uuid>,
    Json(req): Json<TransferQuestionRequest>,
) -> Result<(StatusCode, Json<crate::models::question::QuestionDetail>), (StatusCode, Json<serde_json::Value>)> {
    // ── 1. 加载源题 ──
    let src = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(question_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询源题失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "源题不存在"}))))?;

    // ── 2. 校验对源空间的访问权 ──
    let src_space = get_space(&state.pool, src.space_id)
        .await
        .map_err(|e| db_err(format!("查询源空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "源空间不存在"}))))?;

    if !can_access_space(&state.pool, &auth, &src_space)
        .await
        .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "无权访问源题所在空间"})),
        ));
    }

    // 公共库未发布题：仅管理员可克隆（防止未发布题目被提前扩散）
    if src_space.kind == SpaceKind::Public
        && src.status != QuestionStatus::Published
        && !is_admin_user(&auth)
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "仅可克隆公共库中已发布的题目"})),
        ));
    }

    // ── 3. 解析目标空间（缺省为当前用户个人空间） ──
    let target_space_id = match req.target_space_id {
        Some(tid) => tid,
        None => {
            let display = sqlx::query_scalar::<_, String>(
                "SELECT display_name FROM users WHERE id = $1",
            )
            .bind(auth.id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|e| db_err(format!("查询用户失败: {}", e)))?
            .unwrap_or_else(|| "用户".into());
            ensure_personal_space(&state.pool, auth.id, &display)
                .await
                .map_err(|e| db_err(format!("创建个人空间失败: {}", e)))?
        }
    };

    let target_space = get_space(&state.pool, target_space_id)
        .await
        .map_err(|e| db_err(format!("查询目标空间失败: {}", e)))?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "目标空间不存在"}))))?;

    // ── 4. 目标空间写入权限校验 ──
    // 公共库只能通过「贡献」接口写入，禁止直接克隆
    if target_space.kind == SpaceKind::Public {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "公共库只能通过贡献接口写入"})),
        ));
    }

    if !crate::auth::permissions::can_write_in_space(&state.pool, &auth, &target_space)
        .await
        .map_err(|e| db_err(format!("权限检查失败: {}", e)))?
    {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "无权写入目标空间"})),
        ));
    }

    // ── 5. 事务内深拷贝（强制 Draft + origin_question_id） ──
    let new_id = clone_question_internal(
        &state.pool,
        &src,
        target_space_id,
        auth.id,
        Some(src.id),
    )
    .await
    .map_err(|e| db_err(format!("克隆失败: {}", e)))?;

    // ── 6. 重新查询并构建详情 ──
    let question = sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = $1")
        .bind(new_id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| db_err(format!("查询新题失败: {}", e)))?;

    let detail = build_detail(&state.pool, &auth, question, None)
        .await
        .map_err(|e| db_err(format!("构建详情失败: {}", e)))?;

    Ok((StatusCode::CREATED, Json(detail)))
}

/// 内部：跨空间深拷贝题目（Draft 状态 + 完整字段拷贝 + 知识点/标签关联复制）
///
/// 与 `handlers::questions::copy_question` 的区别：
/// - `copy_question` 用于贡献/导入公共库，status 强制为 'published'
/// - 本函数用于跨空间克隆，status 强制为 'draft'，统计字段清零
/// - 本函数额外复制标签关联（use_count 同步递增），保证字段拷贝完整性
async fn clone_question_internal(
    pool: &sqlx::PgPool,
    src: &Question,
    target_space_id: Uuid,
    creator_id: Uuid,
    origin_id: Option<Uuid>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    let now = chrono::Utc::now();

    let mut tx = pool.begin().await?;

    // ── 拷贝题目本体（除 id/space_id/status/统计/version/审计字段外，全部保留源值） ──
    sqlx::query(
        r#"
        INSERT INTO questions (
            id, stem, stem_text, images, question_type, difficulty, status,
            options, correct_answer, analysis, structure, metadata,
            parent_id, sub_order,
            paper_count, attempt_count, accuracy_rate, favorite_count,
            creator_id, created_at, updated_at, version, space_id, origin_question_id
        )
        VALUES (
            $1, $2, $3, $4, $5, $6, 'draft'::question_status,
            $7, $8, $9, $10, COALESCE($11, '{}'::jsonb),
            $12, $13,
            0, 0, NULL, 0,
            $14, $15, $16, 1, $17, $18
        )
        "#,
    )
    .bind(id)
    .bind(&src.stem)
    .bind(&src.stem_text)
    .bind(&src.images)
    .bind(&src.question_type)
    .bind(&src.difficulty)
    .bind(&src.options)
    .bind(&src.correct_answer)
    .bind(&src.analysis)
    .bind(&src.structure)
    .bind(&src.metadata)
    .bind(src.parent_id)
    .bind(src.sub_order)
    .bind(creator_id)
    .bind(now)
    .bind(now)
    .bind(target_space_id)
    .bind(origin_id)
    .execute(&mut *tx)
    .await?;

    // ── 拷贝知识点节点关联（保留 is_primary/ai_confidence/source） ──
    sqlx::query(
        r#"
        INSERT INTO question_knowledge_nodes (question_id, node_id, is_primary, ai_confidence, source, created_at)
        SELECT $1, node_id, is_primary, ai_confidence, source, NOW()
        FROM question_knowledge_nodes
        WHERE question_id = $2
        "#,
    )
    .bind(id)
    .bind(src.id)
    .execute(&mut *tx)
    .await?;

    // ── 拷贝标签关联（use_count 同步递增） ──
    let tag_ids: Vec<Uuid> =
        sqlx::query_scalar("SELECT tag_id FROM question_tags_relation WHERE question_id = $1")
            .bind(src.id)
            .fetch_all(&mut *tx)
            .await?;

    for tag_id in &tag_ids {
        sqlx::query(
            r#"
            INSERT INTO question_tags_relation (question_id, tag_id)
            VALUES ($1, $2)
            ON CONFLICT DO NOTHING
            "#,
        )
        .bind(id)
        .bind(tag_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query("UPDATE tags SET use_count = use_count + 1 WHERE id = $1")
            .bind(tag_id)
            .execute(&mut *tx)
            .await?;
    }

    // ── 写入版本快照（v1） ──
    save_version(&mut tx, id, 1, Some(creator_id)).await?;

    // ── 提交事务 ──
    tx.commit().await?;
    Ok(id)
}
