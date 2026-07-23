use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// JWT 声明
///
/// 双轨制角色：`role` 为旧枚举（admin/user），`global_role` 为新枚举
/// （super_admin/teacher）。两者同时打入 Token，中间件解析后一并挂载到
/// AuthUser，供业务层做统一判定（见 `is_admin_user`）。
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// 用户 ID
    pub sub: Uuid,
    /// 用户名
    pub username: String,
    /// 旧角色（"admin" / "user"）
    pub role: String,
    /// 新全局角色（"super_admin" / "teacher"）
    pub global_role: String,
    /// 过期时间 (Unix timestamp)
    pub exp: usize,
    /// 签发时间
    pub iat: usize,
}

/// 签发 JWT
///
/// `role` 与 `global_role` 同时打入 Claims，保证中间件能重建完整的
/// 双轨角色上下文，避免旧 `is_admin` 与新 `SuperAdmin` 判定不一致。
pub fn create_token(
    user_id: Uuid,
    username: &str,
    role: &str,
    global_role: &str,
    secret: &str,
    expiry_hours: i64,
) -> Result<String, jsonwebtoken::errors::Error> {
    let now = Utc::now();
    let exp = now + Duration::hours(expiry_hours);

    let claims = Claims {
        sub: user_id,
        username: username.to_string(),
        role: role.to_string(),
        global_role: global_role.to_string(),
        exp: exp.timestamp() as usize,
        iat: now.timestamp() as usize,
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
}

/// 验证 JWT，返回 Claims
pub fn verify_token(token: &str, secret: &str) -> Result<Claims, jsonwebtoken::errors::Error> {
    let token_data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )?;
    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_SECRET: &str = "test-secret-key-for-unit-tests";

    #[test]
    fn test_create_and_verify_token() {
        let user_id = Uuid::new_v4();
        let token = create_token(user_id, "testuser", "User", "teacher", TEST_SECRET, 24)
            .expect("签发 token 失败");

        let claims = verify_token(&token, TEST_SECRET).expect("验证 token 失败");

        assert_eq!(claims.sub, user_id);
        assert_eq!(claims.username, "testuser");
        assert_eq!(claims.role, "User");
        assert_eq!(claims.global_role, "teacher");
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn test_verify_token_wrong_secret() {
        let user_id = Uuid::new_v4();
        let token = create_token(user_id, "testuser", "User", "teacher", TEST_SECRET, 24)
            .expect("签发 token 失败");

        let result = verify_token(&token, "wrong-secret");
        assert!(result.is_err(), "使用错误密钥验证 token 应该失败");
    }

    #[test]
    fn test_verify_invalid_token() {
        let result = verify_token("invalid.token.here", TEST_SECRET);
        assert!(result.is_err(), "无效 token 验证应该失败");
    }

    #[test]
    fn test_token_claims_content() {
        let user_id = Uuid::new_v4();
        let token = create_token(user_id, "alice", "admin", "super_admin", TEST_SECRET, 48)
            .expect("签发 token 失败");

        let claims = verify_token(&token, TEST_SECRET).expect("验证 token 失败");

        assert_eq!(claims.username, "alice");
        assert_eq!(claims.role, "admin");
        assert_eq!(claims.global_role, "super_admin");
        assert_eq!(claims.sub, user_id);
    }
}
