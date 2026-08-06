// ============================================================
// 用户信息页面管理 — Handler 模块
// ------------------------------------------------------------
// 提供当前登录用户的：
//   1. GET    /api/v1/users/me        — 完整个人资料
//   2. PUT    /api/v1/users/me        — 更新基础资料（昵称 / 邮箱）
//   3. PUT    /api/v1/users/password  — 修改密码（成功后前端强制重登）
//   4. POST   /api/v1/users/avatar    — 上传头像（含旧文件清理 + Magic Number 校验）
//
// 安全约束：
//   - 全部接口由全局 require_auth 中间件保护，仅能操作当前 JWT 用户
//   - 文件上传走零信任校验：Content-Type + Magic Bytes 双校验
//   - 文件名由后端生成，绝不使用客户端原文件名（防路径穿越）
//   - 保存新头像后，自动删除该用户旧的头像文件
// ============================================================

use axum::{
    extract::{Extension, Multipart, State},
    http::StatusCode,
    Json,
};
use serde::Serialize;
use serde_json::json;
use std::path::PathBuf;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::models::user::{
    ChangePasswordRequest, GlobalRole, UpdateProfileRequest, User, UserRole, UserQuota,
};
use crate::AppState;

// ---------------------------------------------------------------------------
// 响应类型
// ---------------------------------------------------------------------------

/// 用户完整个人资料（脱敏后，不含 password_hash）
#[derive(Debug, Serialize)]
pub struct UserProfileResponse {
    pub id: Uuid,
    pub username: String,
    pub email: String,
    pub display_name: String,
    pub role: UserRole,
    pub global_role: GlobalRole,
    pub is_active: bool,
    pub avatar_url: Option<String>,
    pub quota: UserQuota,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl From<User> for UserProfileResponse {
    fn from(u: User) -> Self {
        let quota = UserQuota::from(&u);
        Self {
            id: u.id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            role: u.role,
            global_role: u.global_role,
            is_active: u.is_active,
            avatar_url: u.avatar_url,
            quota,
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

/// 头像上传成功响应
#[derive(Debug, Serialize)]
pub struct AvatarUploadResponse {
    pub avatar_url: String,
}

/// 修改密码成功响应
/// 注：后端不强制登出，由前端在收到 200 后清除 token + 跳转登录页
#[derive(Debug, Serialize)]
pub struct PasswordChangeResponse {
    pub message: String,
}

// ---------------------------------------------------------------------------
// 常量与约束
// ---------------------------------------------------------------------------

/// 头像文件最大字节数：2 MB
const MAX_AVATAR_BYTES: usize = 2 * 1024 * 1024;

/// 允许的图片 MIME 类型白名单（pub 供 uploads 模块复用）
pub const ALLOWED_MIME_TYPES: &[&str] = &["image/jpeg", "image/png", "image/webp"];

/// Multipart 表单字段名（前端必须用这个名字）
const AVATAR_FIELD_NAME: &str = "avatar";

/// 头像 URL 前缀（与 lib.rs 中 ServeDir 挂载点保持一致）
const AVATAR_URL_PREFIX: &str = "/uploads/avatars";

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/v1/users/me — 当前登录用户完整资料
///
/// 与 `/auth/me` 相比返回更完整的字段（含 email、quota、updated_at），
/// 用于 Profile 页面渲染。
pub async fn get_my_profile(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
) -> Result<Json<UserProfileResponse>, (StatusCode, Json<serde_json::Value>)> {
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 AND is_active = true")
        .bind(auth.id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("get_my_profile 查询失败 (user_id={}): {:?}", auth.id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("数据库查询失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "用户不存在"}))))?;

    Ok(Json(UserProfileResponse::from(user)))
}

/// PUT /api/v1/users/me — 更新基础资料（display_name / email）
///
/// - username 不允许修改（影响 JWT subject 与登录账号）
/// - email 修改时校验非空、格式正确、不与他人冲突
/// - display_name 校验非空且长度 ≤ 100
pub async fn update_my_profile(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<UpdateProfileRequest>,
) -> Result<Json<UserProfileResponse>, (StatusCode, Json<serde_json::Value>)> {
    // 1. 字段校验
    let new_display_name = req
        .display_name
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    if let Some(ref dn) = new_display_name {
        if dn.chars().count() > 100 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "昵称长度不能超过 100 字符"})),
            ));
        }
    }

    let new_email = req
        .email
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty());
    if let Some(ref email) = new_email {
        if !is_valid_email(email) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "邮箱格式不正确"})),
            ));
        }
    }

    // 2. 邮箱唯一性校验（排除当前用户）
    if let Some(ref email) = new_email {
        let dup_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM users WHERE email = $1 AND id != $2",
        )
        .bind(email)
        .bind(auth.id)
        .fetch_one(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("update_my_profile 邮箱查重失败: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("数据库查询失败: {}", e)})),
            )
        })?;
        if dup_count > 0 {
            return Err((
                StatusCode::CONFLICT,
                Json(json!({"error": "该邮箱已被其他用户使用"})),
            ));
        }
    }

    // 3. 动态拼装 UPDATE 语句 — 仅修改传入字段
    //    使用 COALESCE 模式保证 NULL 字段不变
    let updated_user = sqlx::query_as::<_, User>(
        r#"
        UPDATE users
        SET display_name = COALESCE($1, display_name),
            email        = COALESCE($2, email),
            updated_at   = NOW()
        WHERE id = $3 AND is_active = true
        RETURNING *
        "#,
    )
    .bind(new_display_name.as_deref())
    .bind(new_email.as_deref())
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("update_my_profile 更新失败: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("更新失败: {}", e)})),
        )
    })?
    .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "用户不存在"}))))?;

    Ok(Json(UserProfileResponse::from(updated_user)))
}

/// PUT /api/v1/users/password — 修改密码
///
/// 安全要求：
///   - 旧密码校验：用 `bcrypt::verify` 在 spawn_blocking 中执行，防止阻塞 Tokio worker
///   - 新密码长度 ≥ 8
///   - 新密码不能与旧密码相同
///
/// 前端在收到 200 后必须清除 token 并重定向到登录页（由前端 Profile.vue 实现）
pub async fn change_password(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Json(req): Json<ChangePasswordRequest>,
) -> Result<Json<PasswordChangeResponse>, (StatusCode, Json<serde_json::Value>)> {
    // 1. 新密码格式校验
    if req.new_password.len() < 8 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "新密码长度至少 8 位"})),
        ));
    }
    if req.new_password == req.old_password {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "新密码不能与旧密码相同"})),
        ));
    }

    // 2. 读取当前用户（含 password_hash）
    let user = sqlx::query_as::<_, User>("SELECT * FROM users WHERE id = $1 AND is_active = true")
        .bind(auth.id)
        .fetch_optional(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("change_password 查询用户失败: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("数据库查询失败: {}", e)})),
            )
        })?
        .ok_or_else(|| (StatusCode::NOT_FOUND, Json(json!({"error": "用户不存在"}))))?;

    // 3. 校验旧密码 — bcrypt 是 CPU 密集型，必须卸载到 blocking 线程池
    let old_pw = req.old_password.clone();
    let stored_hash = user.password_hash.clone();
    let valid_old = tokio::task::spawn_blocking(move || {
        bcrypt::verify(&old_pw, &stored_hash).unwrap_or(false)
    })
    .await
    .map_err(|e| {
        tracing::error!("change_password spawn_blocking 失败: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "系统任务调度失败"})),
        )
    })?;

    if !valid_old {
        // 出于安全考虑，不暴露"用户存在"信息，统一返回 401
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "旧密码不正确"})),
        ));
    }

    // 4. 计算新密码 hash — 同样在 blocking 线程池
    let new_pw = req.new_password.clone();
    let new_hash = tokio::task::spawn_blocking(move || {
        bcrypt::hash(&new_pw, bcrypt::DEFAULT_COST)
    })
    .await
    .map_err(|e| {
        tracing::error!("change_password spawn_blocking 失败: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "系统任务调度失败"})),
        )
    })?
    .map_err(|e| {
        tracing::error!("change_password bcrypt hash 失败: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("密码加密失败: {}", e)})),
        )
    })?;

    // 5. 写入新密码 + 更新 updated_at
    sqlx::query("UPDATE users SET password_hash = $1, updated_at = NOW() WHERE id = $2")
        .bind(&new_hash)
        .bind(auth.id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("change_password 更新失败: {:?}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("密码更新失败: {}", e)})),
            )
        })?;

    Ok(Json(PasswordChangeResponse {
        message: "密码修改成功，请使用新密码重新登录".to_string(),
    }))
}

/// POST /api/v1/users/avatar — 上传头像
///
/// 流程：
///   1. 解析 multipart，提取 `avatar` 字段的二进制数据
///   2. 零信任校验：大小 ≤ 2MB + MIME 白名单 + Magic Bytes（infer crate）
///   3. 生成安全文件名：`{user_id}_{timestamp_ms}_{rand8}.{ext}`
///   4. 写入 `{upload_dir}/avatars/{filename}`
///   5. UPDATE users SET avatar_url = '/uploads/avatars/{filename}'
///   6. 若旧 avatar_url 指向本地，异步删除旧文件（失败仅 log）
///   7. 返回新的 avatar_url
pub async fn upload_avatar(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    mut multipart: Multipart,
) -> Result<(StatusCode, Json<AvatarUploadResponse>), (StatusCode, Json<serde_json::Value>)> {
    // 1. 解析 multipart — 提取 avatar 字段
    let mut image_bytes: Option<Vec<u8>> = None;
    let mut client_mime: Option<String> = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| {
            tracing::error!("upload_avatar multipart 解析失败: {:?}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": format!("multipart 解析失败: {}", e)})),
            )
        })?
    {
        let name = field.name().unwrap_or("").to_string();
        if name != AVATAR_FIELD_NAME {
            // 忽略未知字段（不报错，容错）
            continue;
        }

        client_mime = field.content_type().map(|s| s.to_string());
        let bytes = field
            .bytes()
            .await
            .map_err(|e| {
                tracing::error!("upload_avatar 读取字段字节失败: {:?}", e);
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("读取文件数据失败: {}", e)})),
                )
            })?
            .to_vec();
        image_bytes = Some(bytes);
        break;
    }

    let image_bytes = image_bytes.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({"error": format!("未接收到 {} 字段", AVATAR_FIELD_NAME)})),
        )
    })?;

    // 2. 大小校验
    if image_bytes.len() > MAX_AVATAR_BYTES {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            Json(json!({
                "error": format!("头像文件过大，最大允许 {} 字节", MAX_AVATAR_BYTES),
                "limit": MAX_AVATAR_BYTES,
            })),
        ));
    }

    // 3. 零信任类型校验：MIME 白名单 + Magic Bytes
    let ext = validate_image_type(&image_bytes, client_mime.as_deref()).ok_or_else(|| {
        (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(json!({
                "error": "仅支持 jpg / png / webp 格式",
                "allowed": ALLOWED_MIME_TYPES,
            })),
        )
    })?;

    // 4. 生成安全文件名 — 不使用客户端原文件名（防路径穿越）
    let timestamp = chrono::Utc::now().timestamp_millis();
    let rand_suffix = Uuid::new_v4().simple().to_string();
    let rand_suffix = &rand_suffix[..8];
    let filename = format!("{}_{}_{}.{}", auth.id, timestamp, rand_suffix, ext);

    // 5. 写入文件系统
    let upload_dir = PathBuf::from(&state.upload_dir);
    let avatars_dir = upload_dir.join("avatars");
    let file_path = avatars_dir.join(&filename);

    // 确保目录存在（双保险 — main.rs 启动时已创建）
    if let Err(e) = tokio::fs::create_dir_all(&avatars_dir).await {
        tracing::error!("upload_avatar 创建目录失败 {:?}: {:?}", avatars_dir, e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("创建目录失败: {}", e)})),
        ));
    }

    if let Err(e) = tokio::fs::write(&file_path, &image_bytes).await {
        tracing::error!("upload_avatar 写入文件失败 {:?}: {:?}", file_path, e);
        return Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("写入文件失败: {}", e)})),
        ));
    }

    let new_avatar_url = format!("{}/{}", AVATAR_URL_PREFIX, filename);

    // 6. 读取旧 avatar_url，更新 DB
    let old_avatar_url: Option<String> = sqlx::query_scalar(
        "SELECT avatar_url FROM users WHERE id = $1",
    )
    .bind(auth.id)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        tracing::error!("upload_avatar 读取旧 avatar_url 失败: {:?}", e);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("数据库查询失败: {}", e)})),
        )
    })?;

    sqlx::query("UPDATE users SET avatar_url = $1, updated_at = NOW() WHERE id = $2")
        .bind(&new_avatar_url)
        .bind(auth.id)
        .execute(&state.pool)
        .await
        .map_err(|e| {
            tracing::error!("upload_avatar 更新数据库失败: {:?}", e);
            // DB 更新失败也要清理刚写入的文件
            let fp = file_path.clone();
            tokio::spawn(async move {
                let _ = tokio::fs::remove_file(&fp).await;
            });
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("更新头像失败: {}", e)})),
            )
        })?;

    // 7. 旧文件清理 — 仅删除本地头像（指向 /uploads/avatars/）
    //    用 spawn 隔离，失败仅 log，不阻断当前请求
    if let Some(old_url) = old_avatar_url {
        if let Some(old_filename) = old_url
            .strip_prefix(&format!("{}/", AVATAR_URL_PREFIX))
            .filter(|s| !s.is_empty() && !s.contains('/') && !s.contains(".."))
        {
            let old_path = avatars_dir.join(old_filename);
            tokio::spawn(async move {
                match tokio::fs::remove_file(&old_path).await {
                    Ok(()) => {
                        tracing::info!("已清理旧头像文件: {:?}", old_path);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        // 旧文件已不存在 — 静默忽略
                    }
                    Err(e) => {
                        tracing::warn!("清理旧头像失败 {:?}: {:?}", old_path, e);
                    }
                }
            });
        }
    }

    Ok((
        StatusCode::OK,
        Json(AvatarUploadResponse {
            avatar_url: new_avatar_url,
        }),
    ))
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 简易邮箱格式校验（避免引入额外 regex crate — 虽然已有 regex 1）
/// 规则：必须含 @，@ 前后非空，域名含至少一个 .
fn is_valid_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    if parts.len() != 2 {
        return false;
    }
    let (local, domain) = (parts[0], parts[1]);
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
}

/// 零信任图片类型校验 — 同时校验 MIME 头与 Magic Bytes
///
/// 返回 Some(扩展名) 当且仅当：
///   - 客户端 Content-Type（若存在）在白名单内
///   - 文件内容 Magic Bytes 识别为 jpg/png/webp
///   - 两者一致
///
/// 失败返回 None，由调用方返回 415 Unsupported Media Type
pub fn validate_image_type(bytes: &[u8], client_mime: Option<&str>) -> Option<&'static str> {
    // 用 infer crate 识别 Magic Bytes
    let detected_kind = infer::get(bytes)?;

    let (real_mime, ext) = match detected_kind.mime_type() {
        "image/jpeg" => ("image/jpeg", "jpg"),
        "image/png" => ("image/png", "png"),
        "image/webp" => ("image/webp", "webp"),
        _ => return None,
    };

    // 校验白名单（防御冗余 — infer 已限定到三种，但白名单是单一信息源）
    if !ALLOWED_MIME_TYPES.contains(&real_mime) {
        return None;
    }

    // 若客户端声明了 Content-Type，必须与 Magic Bytes 一致
    if let Some(declared) = client_mime {
        if declared != real_mime {
            tracing::warn!(
                "头像 MIME 不匹配: declared={}, real={}",
                declared,
                real_mime
            );
            return None;
        }
    }

    Some(ext)
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_valid_email() {
        // 合法邮箱
        assert!(is_valid_email("user@example.com"));
        assert!(is_valid_email("john.doe@sub.example.cn"));
        assert!(is_valid_email("a@b.co"));

        // 非法邮箱
        assert!(!is_valid_email("user@example"));        // 无顶级域名
        assert!(!is_valid_email("@example.com"));         // 无本地部分
        assert!(!is_valid_email("user@"));                 // 无域名
        assert!(!is_valid_email("userexample.com"));      // 无 @
        assert!(!is_valid_email("user@.com"));             // 域名以 . 开头
        assert!(!is_valid_email("user@example."));         // 域名以 . 结尾
        assert!(!is_valid_email("user@a@b.com"));          // 多个 @
    }

    #[test]
    fn test_validate_image_type_jpeg() {
        // JPEG 文件头：FF D8 FF
        let jpeg_header = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'];
        // 补一些零字节让 infer 能识别
        let mut jpeg = jpeg_header.to_vec();
        jpeg.extend(std::iter::repeat(0).take(50));
        let ext = validate_image_type(&jpeg, Some("image/jpeg"));
        assert_eq!(ext, Some("jpg"));
    }

    #[test]
    fn test_validate_image_type_png() {
        // PNG 文件头：89 50 4E 47 0D 0A 1A 0A
        let png_header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let mut png = png_header.to_vec();
        png.extend(std::iter::repeat(0).take(50));
        let ext = validate_image_type(&png, Some("image/png"));
        assert_eq!(ext, Some("png"));
    }

    #[test]
    fn test_validate_image_type_mime_mismatch_returns_none() {
        // 客户端声明 jpeg，实际是 png → 拒绝
        let png_header = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let mut png = png_header.to_vec();
        png.extend(std::iter::repeat(0).take(50));
        let ext = validate_image_type(&png, Some("image/jpeg"));
        assert_eq!(ext, None);
    }

    #[test]
    fn test_validate_image_type_non_image_returns_none() {
        // 一段纯文本
        let text = b"hello world not an image";
        let ext = validate_image_type(text, Some("image/jpeg"));
        assert_eq!(ext, None);
    }
}
