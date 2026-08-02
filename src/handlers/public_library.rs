use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde_json::json;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::{ensure_public_space, get_space, is_admin_user};
use crate::handlers::notifications::send_notification;
use crate::models::notification::CreateNotification;
use crate::models::question::{PublicLibrarySubmissionDetail, SubmissionStatus};
use crate::models::space::SpaceKind;
use crate::AppState;

// ===========================================================================
// 请求体
// ===========================================================================

#[derive(serde::Deserialize)]
pub struct SubmitToPublicRequest {
    pub comment: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct ReviewSubmissionRequest {
    pub action: String,           // "approved" | "rejected"
    pub review_comment: Option<String>,
}

// ===========================================================================
// POST /api/v1/questions/:id/submit-to-public — 提交推库申请
// ===========================================================================

pub async fn submit_to_public(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(question_id): Path<Uuid>,
    Json(req): Json<SubmitToPublicRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    // 查询题目
    let question: Option<(Uuid, Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, space_id, creator_id, status::text FROM questions WHERE id = $1",
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal_err(format!("查询题目失败: {}", e)))?;

    let (_, space_id, creator_id, status) =
        question.ok_or_else(|| not_found("题目不存在"))?;

    // 必须已发布（空间内部审核通过）
    if status != "published" {
        return Err(bad_request("仅已审核通过的题目可推送到公共题库"));
    }

    // 查询空间
    let space = get_space(&state.pool, space_id)
        .await
        .map_err(|e| internal_err(format!("查询空间失败: {}", e)))?
        .ok_or_else(|| not_found("空间不存在"))?;

    // 公共空间题目无需推送
    if space.kind == SpaceKind::Public {
        return Err(bad_request("该题目已在公共空间中"));
    }

    // 权限：creator 或 admin
    if creator_id != auth.id && !is_admin_user(&auth) {
        return Err(forbidden("仅题目创建者可推送至公共题库"));
    }

    // 防重复：检查是否已有 pending 申请
    let existing_pending: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM public_library_submissions WHERE question_id = $1 AND status = 'pending'",
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal_err(format!("检查重复申请失败: {}", e)))?;

    if existing_pending.is_some() {
        return Err(conflict("该题目已有待审核的推库申请，请勿重复提交"));
    }

    // 创建申请记录
    let submission_id: Uuid = sqlx::query_scalar(
        r#"
        INSERT INTO public_library_submissions (question_id, source_space_id, submitted_by, status)
        VALUES ($1, $2, $3, 'pending'::submission_status)
        RETURNING id
        "#,
    )
    .bind(question_id)
    .bind(space_id)
    .bind(auth.id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| internal_err(format!("创建推库申请失败: {}", e)))?;

    // 通知所有超级管理员
    let admin_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM users WHERE global_role = 'super_admin' AND is_active = true",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default();

    for admin_id in admin_ids {
        if let Err(e) = send_notification(
            &state.pool,
            &state.notify_tx,
            CreateNotification {
                user_id: admin_id,
                kind: "system".into(),
                title: "新的推库申请".into(),
                body: Some(format!(
                    "有新的题目申请推送到公共题库，来自空间「{}」",
                    space.name
                )),
                resource_type: Some("question".into()),
                resource_id: Some(question_id),
            },
        )
        .await
        {
            tracing::warn!("推库通知发送失败, admin_id={}, err={}", admin_id, e);
        }
    }

    tracing::info!(
        "推库申请已创建: submission_id={}, question_id={}, space={}",
        submission_id,
        question_id,
        space.name
    );

    Ok(StatusCode::CREATED)
}

// ===========================================================================
// DELETE /api/v1/public-library/:id — 撤回推库申请（仅发起人）
// ===========================================================================

pub async fn withdraw_submission(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(submission_id): Path<Uuid>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    let result = sqlx::query(
        "DELETE FROM public_library_submissions WHERE id = $1 AND submitted_by = $2 AND status = 'pending'",
    )
    .bind(submission_id)
    .bind(auth.id)
    .execute(&state.pool)
    .await
    .map_err(|e| internal_err(format!("撤回失败: {}", e)))?;

    if result.rows_affected() == 0 {
        return Err(not_found("申请不存在、已处理或无权操作"));
    }

    Ok(StatusCode::NO_CONTENT)
}

// ===========================================================================
// GET /api/v1/public-library/pending — 管理员查看推库待审列表
// ===========================================================================

pub async fn list_pending(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<Vec<PublicLibrarySubmissionDetail>>, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth) {
        return Err(forbidden("仅管理员可查看推库审批列表"));
    }

    let rows = sqlx::query_as::<_, PublicLibrarySubmissionDetail>(
        r#"
        SELECT
            pls.id, pls.question_id, pls.source_space_id,
            s.name AS source_space_name,
            pls.submitted_by,
            u.username AS submitter_name,
            pls.status, pls.review_comment, pls.reviewed_by, pls.reviewed_at,
            pls.created_at,
            q.stem, q.question_type, q.difficulty
        FROM public_library_submissions pls
        JOIN questions q ON q.id = pls.question_id
        JOIN spaces s ON s.id = pls.source_space_id
        JOIN users u ON u.id = pls.submitted_by
        WHERE pls.status = 'pending'::submission_status
        ORDER BY pls.created_at ASC
        "#,
    )
    .fetch_all(&state.pool)
    .await
    .map_err(|e| internal_err(format!("查询推库列表失败: {}", e)))?;

    Ok(Json(rows))
}

// ===========================================================================
// POST /api/v1/public-library/:id/review — 管理员审核推库申请
// ===========================================================================

pub async fn review_submission(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(submission_id): Path<Uuid>,
    Json(req): Json<ReviewSubmissionRequest>,
) -> Result<StatusCode, (StatusCode, Json<serde_json::Value>)> {
    if !is_admin_user(&auth) {
        return Err(forbidden("仅管理员可审核推库申请"));
    }

    // 查询申请记录
    let submission: Option<(Uuid, Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT id, question_id, source_space_id, submitted_by FROM public_library_submissions WHERE id = $1 AND status = 'pending'",
    )
    .bind(submission_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal_err(format!("查询申请失败: {}", e)))?;

    let (_, question_id, source_space_id, submitted_by) =
        submission.ok_or_else(|| not_found("申请不存在或已处理"))?;

    let now = chrono::Utc::now();
    let action = req.action.as_str();

    if action == "approved" {
        // ── 事务：复制题目到公共空间 + 更新申请状态 ──
        let public_space_id = ensure_public_space(&state.pool)
            .await
            .map_err(|e| internal_err(format!("获取公共空间失败: {}", e)))?;

        let mut tx = state.pool.begin().await.map_err(|e| internal_err(format!("开启事务失败: {}", e)))?;

        // 复制题目到公共空间（新 ID，保留血缘：origin_question_id = 原题 ID）
        // 列与 contribute_to_public 保持一致，避免列不存在或类型不匹配
        sqlx::query(
            r#"
            INSERT INTO questions (
                id, stem, stem_text, images, question_type, difficulty, status,
                options, correct_answer, analysis, metadata,
                parent_id, sub_order,
                creator_id, created_at, updated_at, version, space_id, origin_question_id
            )
            SELECT
                gen_random_uuid(), stem, stem_text, images, question_type, difficulty,
                'published'::question_status,
                options, correct_answer, analysis, metadata,
                parent_id, sub_order,
                creator_id, $2, $2, 1, $1, $3
            FROM questions WHERE id = $3
            "#,
        )
        .bind(public_space_id)
        .bind(now)
        .bind(question_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal_err(format!("复制题目失败: {}", e)))?;

        // 更新申请状态
        sqlx::query(
            r#"
            UPDATE public_library_submissions
            SET status = 'approved'::submission_status, reviewed_by = $1, reviewed_at = $2, review_comment = $3
            WHERE id = $4
            "#,
        )
        .bind(auth.id)
        .bind(now)
        .bind(&req.review_comment)
        .bind(submission_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal_err(format!("更新申请状态失败: {}", e)))?;

        tx.commit().await.map_err(|e| internal_err(format!("提交事务失败: {}", e)))?;

        // 通知申请人
        let _ = send_notification(
            &state.pool,
            &state.notify_tx,
            CreateNotification {
                user_id: submitted_by,
                kind: "system".into(),
                title: "推库申请已通过".into(),
                body: Some("您的题目已成功推送到公共题库".into()),
                resource_type: Some("question".into()),
                resource_id: Some(question_id),
            },
        ).await;

    } else if action == "rejected" {
        // ── 驳回：事务操作，回退原题目状态并记录元数据（GAP-5） ──
        let mut tx = state.pool.begin().await.map_err(|e| internal_err(format!("开启事务失败: {}", e)))?;

        // 1. 获取来源空间类型，用于精确追溯
        let space_kind: String = sqlx::query_scalar("SELECT kind::text FROM spaces WHERE id = $1")
            .bind(source_space_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| internal_err(format!("查询来源空间失败: {}", e)))?;

        // 2. 更新申请状态
        sqlx::query(
            r#"
            UPDATE public_library_submissions
            SET status = 'rejected'::submission_status, reviewed_by = $1, reviewed_at = $2, review_comment = $3
            WHERE id = $4
            "#,
        )
        .bind(auth.id)
        .bind(now)
        .bind(&req.review_comment)
        .bind(submission_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal_err(format!("更新申请状态失败: {}", e)))?;

        // 3. 严格状态回退与元数据标记
        // 当前使用 'published' 代表原空间的“已审核”状态。
        // （注：待 GAP-6 状态枚举重构后，这里将替换为具体的 'approved_personal' 或 'approved_team'）
        let reject_meta = json!({
            "rejected_from": "public_library",
            "source_space_kind": space_kind,
            "rejected_at": now.to_rfc3339(),
            "reason": req.review_comment.clone().unwrap_or_default(),
            "reviewer_id": auth.id,
        });

        sqlx::query(
            r#"
            UPDATE questions
            SET status = 'published'::question_status,
                metadata = jsonb_set(
                    COALESCE(metadata, '{}'::jsonb),
                    '{public_library_rejected}',
                    $1::jsonb
                )
            WHERE id = $2
            "#,
        )
        .bind(reject_meta)
        .bind(question_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| internal_err(format!("更新原题目状态及元数据失败: {}", e)))?;

        tx.commit().await.map_err(|e| internal_err(format!("提交事务失败: {}", e)))?;

        // 4. 通知申请人
        let _ = send_notification(
            &state.pool,
            &state.notify_tx,
            CreateNotification {
                user_id: submitted_by,
                kind: "system".into(),
                title: "推库申请被驳回".into(),
                body: Some(format!(
                    "您的推库申请已被驳回{}",
                    req.review_comment.as_ref().map(|c| format!("：{}", c)).unwrap_or_default()
                )),
                resource_type: Some("question".into()),
                resource_id: Some(question_id),
            },
        ).await;

    } else {
        return Err(bad_request("无效的审核操作，请使用 approved 或 rejected"));
    }

    Ok(StatusCode::OK)
}

// ===========================================================================
// GET /api/v1/questions/:id/public-submission — 查询题目的推库申请状态
// ===========================================================================

pub async fn get_question_submission_status(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Path(question_id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    let row: Option<(Uuid, String)> = sqlx::query_as(
        "SELECT id, status::text FROM public_library_submissions WHERE question_id = $1 AND status = 'pending'",
    )
    .bind(question_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| internal_err(format!("查询推库状态失败: {}", e)))?;

    Ok(Json(json!({
        "has_pending_submission": row.is_some(),
        "submission_id": row.map(|(id, _)| id),
    })))
}

// ===========================================================================
// 错误辅助
// ===========================================================================

fn internal_err(msg: String) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": msg})))
}
fn not_found(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::NOT_FOUND, Json(json!({"error": msg})))
}
fn bad_request(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(json!({"error": msg})))
}
fn forbidden(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::FORBIDDEN, Json(json!({"error": msg})))
}
fn conflict(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::CONFLICT, Json(json!({"error": msg})))
}
