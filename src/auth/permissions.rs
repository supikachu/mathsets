use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use sqlx::PgPool;
use thiserror::Error;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::models::question::{Question, QuestionStatus};
use crate::models::space::{Space, SpaceKind, SpaceSettings};
use crate::models::user::{GlobalRole, User};

/// 系统管理员（兼容旧 role 字符串）
pub fn is_admin(role: &str) -> bool {
    role == "Admin" || role.eq_ignore_ascii_case("admin")
}

/// 加载空间
pub async fn get_space(pool: &PgPool, space_id: Uuid) -> Result<Option<Space>, sqlx::Error> {
    sqlx::query_as::<_, Space>("SELECT * FROM spaces WHERE id = $1")
        .bind(space_id)
        .fetch_optional(pool)
        .await
}

/// 获取用户个人空间 id（不存在则创建）
pub async fn ensure_personal_space(
    pool: &PgPool,
    user_id: Uuid,
    display_name: &str,
) -> Result<Uuid, sqlx::Error> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>(
        "SELECT id FROM spaces WHERE kind = 'personal' AND owner_user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    {
        return Ok(id);
    }

    let id = Uuid::new_v4();
    let name = format!("{} 的题库", display_name);
    let settings = serde_json::json!({
        "allow_creator_self_review": true,
        "require_review_duty": false
    });
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO spaces (id, kind, name, owner_user_id, settings, created_at, updated_at)
        VALUES ($1, 'personal', $2, $3, $4, $5, $6)
        "#,
    )
    .bind(id)
    .bind(&name)
    .bind(user_id)
    .bind(&settings)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(id)
}

/// 确保公共空间存在，返回 id
pub async fn ensure_public_space(pool: &PgPool) -> Result<Uuid, sqlx::Error> {
    if let Some(id) = sqlx::query_scalar::<_, Uuid>("SELECT id FROM spaces WHERE kind = 'public' LIMIT 1")
        .fetch_optional(pool)
        .await?
    {
        return Ok(id);
    }

    let id = Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap();
    let settings = serde_json::json!({
        "allow_creator_self_review": false,
        "require_review_duty": false
    });
    let now = chrono::Utc::now();

    sqlx::query(
        r#"
        INSERT INTO spaces (id, kind, name, owner_user_id, settings, created_at, updated_at)
        VALUES ($1, 'public', '公共题库', NULL, $2, $3, $4)
        ON CONFLICT DO NOTHING
        "#,
    )
    .bind(id)
    .bind(&settings)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;

    Ok(id)
}

/// 用户是否可访问该空间（查看题目列表/详情）
pub async fn can_access_space(
    pool: &PgPool,
    auth: &AuthUser,
    space: &Space,
) -> Result<bool, sqlx::Error> {
    if is_admin(&auth.role) {
        return Ok(true);
    }
    match space.kind {
        SpaceKind::Public => Ok(true),
        SpaceKind::Personal => Ok(space.owner_user_id == Some(auth.id)),
        SpaceKind::Team => is_space_member(pool, space.id, auth.id).await,
    }
}

/// 是否为团队空间成员（或个人空间所有者）
pub async fn is_space_member(pool: &PgPool, space_id: Uuid, user_id: Uuid) -> Result<bool, sqlx::Error> {
    let space = match get_space(pool, space_id).await? {
        Some(s) => s,
        None => return Ok(false),
    };

    match space.kind {
        SpaceKind::Public => Ok(true),
        SpaceKind::Personal => Ok(space.owner_user_id == Some(user_id)),
        SpaceKind::Team => {
            let exists = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM space_members WHERE space_id = $1 AND user_id = $2",
            )
            .bind(space_id)
            .bind(user_id)
            .fetch_one(pool)
            .await?;
            Ok(exists > 0)
        }
    }
}

/// 空间内成员角色与职责
pub async fn get_member_meta(
    pool: &PgPool,
    space_id: Uuid,
    user_id: Uuid,
) -> Result<Option<(String, Vec<String>)>, sqlx::Error> {
    let space = match get_space(pool, space_id).await? {
        Some(s) => s,
        None => return Ok(None),
    };

    match space.kind {
        SpaceKind::Personal if space.owner_user_id == Some(user_id) => {
            Ok(Some(("owner".into(), vec![])))
        }
        SpaceKind::Public => Ok(Some(("member".into(), vec![]))),
        SpaceKind::Team => {
            let row = sqlx::query_as::<_, (String, Vec<String>)>(
                "SELECT role, duties FROM space_members WHERE space_id = $1 AND user_id = $2",
            )
            .bind(space_id)
            .bind(user_id)
            .fetch_optional(pool)
            .await?;
            Ok(row)
        }
        _ => Ok(None),
    }
}

/// 是否可在空间内创建/编辑题目（成员）
pub async fn can_write_in_space(
    pool: &PgPool,
    auth: &AuthUser,
    space: &Space,
) -> Result<bool, sqlx::Error> {
    if is_admin(&auth.role) {
        return Ok(true);
    }
    match space.kind {
        SpaceKind::Public => Ok(false), // 公共库仅通过「贡献」写入
        SpaceKind::Personal => Ok(space.owner_user_id == Some(auth.id)),
        SpaceKind::Team => is_space_member(pool, space.id, auth.id).await,
    }
}

/// 是否可编辑该题（草稿/驳回 + 创建者或空间 owner）
pub async fn can_edit_question(
    pool: &PgPool,
    auth: &AuthUser,
    space: &Space,
    creator_id: Option<Uuid>,
    status: &QuestionStatus,
) -> Result<bool, sqlx::Error> {
    if *status != QuestionStatus::Draft && *status != QuestionStatus::Rejected {
        return Ok(false);
    }
    if is_admin(&auth.role) {
        return Ok(true);
    }
    if !can_write_in_space(pool, auth, space).await? {
        return Ok(false);
    }
    if creator_id == Some(auth.id) {
        return Ok(true);
    }
    // 团队 owner 可协助编辑
    if let Some((role, _)) = get_member_meta(pool, space.id, auth.id).await? {
        return Ok(role == "owner");
    }
    Ok(false)
}

/// 指定审题人列表
pub async fn list_reviewers(pool: &PgPool, question_id: Uuid) -> Result<Vec<Uuid>, sqlx::Error> {
    sqlx::query_scalar::<_, Uuid>(
        "SELECT user_id FROM question_reviewers WHERE question_id = $1",
    )
    .bind(question_id)
    .fetch_all(pool)
    .await
}

/// 是否可审核该待审题
pub async fn can_review_question(
    pool: &PgPool,
    auth: &AuthUser,
    space: &Space,
    creator_id: Option<Uuid>,
    status: &QuestionStatus,
    question_id: Uuid,
) -> Result<bool, sqlx::Error> {
    if *status != QuestionStatus::Pending {
        return Ok(false);
    }
    if is_admin(&auth.role) {
        return Ok(true);
    }

    let reviewers = list_reviewers(pool, question_id).await?;
    if !reviewers.is_empty() {
        return Ok(reviewers.contains(&auth.id));
    }

    let settings: SpaceSettings = space.settings_parsed();

    match space.kind {
        SpaceKind::Personal => {
            // 个人空间：默认创建者自审
            Ok(creator_id == Some(auth.id) && settings.allow_creator_self_review)
        }
        SpaceKind::Team => {
            if !is_space_member(pool, space.id, auth.id).await? {
                return Ok(false);
            }
            if settings.require_review_duty {
                if let Some((_, duties)) = get_member_meta(pool, space.id, auth.id).await? {
                    if !duties.iter().any(|d| d == "review") {
                        return Ok(false);
                    }
                } else {
                    return Ok(false);
                }
            }
            // 团队空间：创建者回避，不能审自己录入的题
            if creator_id == Some(auth.id) {
                return Ok(false);
            }
            Ok(true)
        }
        SpaceKind::Public => Ok(false),
    }
}

// ---------------------------------------------------------------------------
// 发布权限（Maker-Checker 核心引擎）
// ---------------------------------------------------------------------------

/// 发布鉴权业务错误 — 与 sqlx::Error 区分，便于 handler 直接 `?` 传播并统一响应
#[derive(Debug, Error)]
pub enum PermissionError {
    /// 录审分离违规：团队空间下创建者尝试审核自己的题目
    #[error("录审分离违规：创建者不能审核自己录入的题目")]
    MakerCheckerViolation,

    /// 缺少所需权限（携带具体权限标识，便于前端展示）
    #[error("无权操作：缺少 {0} 权限")]
    MissingPrivilege(String),

    /// 数据库查询失败（内部错误，不暴露细节给前端）
    #[error("数据库查询失败")]
    Database(#[from] sqlx::Error),
}

impl IntoResponse for PermissionError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            PermissionError::MakerCheckerViolation => (
                StatusCode::FORBIDDEN,
                "ERR_MAKER_CHECKER_VIOLATION",
            ),
            PermissionError::MissingPrivilege(_) => (StatusCode::FORBIDDEN, "ERR_FORBIDDEN"),
            PermissionError::Database(_) => {
                tracing::error!("权限检查数据库错误: {:?}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "ERR_INTERNAL_SERVER",
                )
            }
        };
        let message = self.to_string();
        (status, Json(json!({ "error": message, "code": code }))).into_response()
    }
}

/// 是否可发布该题（Maker-Checker 核心鉴权）
///
/// 教研流转法则（严密不漏）：
/// 1. `GlobalRole::SuperAdmin` 一票通过权（最高优先级，直接返回 true）
/// 2. `SpaceKind::Personal`：允许自审 — 仅当 `question.creator_id == user.id`
///    且空间 settings.allow_creator_self_review 为 true
/// 3. `SpaceKind::Team`：强制录审分离
///    - 若 `question.creator_id == user.id` → 返回 `Err(MakerCheckerViolation)`
///    - 否则查询当前用户在该 space 的角色，要求具有 Owner 或 Reviewer 权限
/// 4. `SpaceKind::Public`：公共库仅可通过「贡献」接口写入，禁止直接发布
pub async fn can_publish_question(
    pool: &PgPool,
    user: &User,
    question: &Question,
    space: &Space,
) -> Result<bool, PermissionError> {
    // ── 法则 1：SuperAdmin 一票通过 ──
    if user.global_role == GlobalRole::SuperAdmin {
        return Ok(true);
    }

    match space.kind {
        SpaceKind::Personal => {
            // ── 法则 2：个人空间自审 ──
            let settings = space.settings_parsed();
            if question.creator_id == user.id && settings.allow_creator_self_review {
                Ok(true)
            } else {
                Err(PermissionError::MissingPrivilege("personal_self_review".into()))
            }
        }
        SpaceKind::Team => {
            // ── 法则 3a：录审分离硬拦截 ──
            if question.creator_id == user.id {
                return Err(PermissionError::MakerCheckerViolation);
            }

            // ── 法则 3b：查询当前用户在该 space 的角色 ──
            let role_str: Option<String> = sqlx::query_scalar(
                "SELECT role FROM space_members WHERE space_id = $1 AND user_id = $2",
            )
            .bind(space.id)
            .bind(user.id)
            .fetch_optional(pool)
            .await?;

            match role_str.as_deref() {
                Some("owner") | Some("reviewer") => Ok(true),
                Some(other) => Err(PermissionError::MissingPrivilege(format!(
                    "space_role={}",
                    other
                ))),
                None => {
                    Err(PermissionError::MissingPrivilege("space_member".into()))
                }
            }
        }
        SpaceKind::Public => {
            // ── 法则 4：公共库禁止直接发布 ──
            Err(PermissionError::MissingPrivilege("public_publish".into()))
        }
    }
}
