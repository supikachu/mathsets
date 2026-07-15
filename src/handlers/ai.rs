use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::ai::cleaner::clean_and_parse;
use crate::ai::kp_matcher::match_knowledge_points;
use crate::ai::provider::{create_provider, AiError};
use crate::ai::types::ParsedQuestion;
use crate::auth::middleware::AuthUser;
use crate::models::ai_setting::{
    decrypt_api_key, encrypt_api_key, parse_master_key, AiSettingsResponse, UpdateAiSettingsRequest,
    UserAiSetting,
};
use crate::AppState;

// ---------------------------------------------------------------------------
// 请求/响应类型
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ParseTextRequest {
    pub text: String,
}

#[derive(Serialize)]
pub struct ParseResponse {
    pub data: ParsedQuestion,
}

/// 获取 AI 配置
pub async fn get_settings(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
) -> Result<Json<AiSettingsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let setting = sqlx::query_as::<_, UserAiSetting>(
        "SELECT * FROM user_ai_settings WHERE user_id = $1",
    )
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("数据库查询失败: {e}")})),
        )
    })?;

    let resp = match setting {
        Some(s) => AiSettingsResponse {
            provider: s.provider,
            has_api_key: s.api_key_enc.is_some(),
            model_text: s.model_text,
            model_vision: s.model_vision,
        },
        None => AiSettingsResponse {
            provider: "deepseek".to_string(),
            has_api_key: false,
            model_text: None,
            model_vision: None,
        },
    };

    Ok(Json(resp))
}

/// 更新 AI 配置
pub async fn update_settings(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
    Json(req): Json<UpdateAiSettingsRequest>,
) -> Result<Json<AiSettingsResponse>, (StatusCode, Json<serde_json::Value>)> {
    let master_key = match &state.ai_config.key_encryption_key {
        Some(k) => match parse_master_key(k) {
            Ok(mk) => mk,
            Err(e) => {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("服务器密钥配置错误: {e}")})),
                ))
            }
        },
        None => {
            return Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "服务器未配置 AI_KEY_ENCRYPTION_KEY"})),
            ))
        }
    };

    // 加密 API Key（若提供）
    let (api_key_enc, api_key_iv) = match &req.api_key {
        Some(key) if !key.is_empty() => {
            let (enc, iv) = encrypt_api_key(key, &master_key).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("加密失败: {e}")})),
                )
            })?;
            (Some(enc), Some(iv))
        }
        _ => (None, None),
    };

    // UPSERT
    let provider = req.provider.unwrap_or_else(|| "deepseek".to_string());
    let setting = sqlx::query_as::<_, UserAiSetting>(
        r#"
        INSERT INTO user_ai_settings (user_id, provider, api_key_enc, api_key_iv, model_text, model_vision, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        ON CONFLICT (user_id) DO UPDATE SET
            provider = COALESCE($2, user_ai_settings.provider),
            api_key_enc = CASE WHEN $3 IS NOT NULL THEN $3 ELSE user_ai_settings.api_key_enc END,
            api_key_iv = CASE WHEN $4 IS NOT NULL THEN $4 ELSE user_ai_settings.api_key_iv END,
            model_text = $5,
            model_vision = $6,
            updated_at = NOW()
        RETURNING *
        "#,
    )
    .bind(auth.id)
    .bind(&provider)
    .bind(&api_key_enc)
    .bind(&api_key_iv)
    .bind(&req.model_text)
    .bind(&req.model_vision)
    .fetch_one(&state.pool)
    .await
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("数据库保存失败: {e}")})),
        )
    })?;

    let resp = AiSettingsResponse {
        provider: setting.provider,
        has_api_key: setting.api_key_enc.is_some(),
        model_text: setting.model_text,
        model_vision: setting.model_vision,
    };

    Ok(Json(resp))
}

/// 文本解析
pub async fn parse_text(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
    Json(req): Json<ParseTextRequest>,
) -> Result<Json<ParseResponse>, (StatusCode, Json<serde_json::Value>)> {
    if req.text.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "文本内容不能为空"})),
        ));
    }

    // 获取 API Key（用户个人优先，否则平台默认）
    let (api_key, provider_name, model, base_url) =
        resolve_ai_config(&auth, &state).await.map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": e})),
            )
        })?;

    // 创建 provider 并调用
    let provider = create_provider(&provider_name, &api_key, &base_url);
    let raw_json = provider
        .parse_text(&req.text, model.as_deref())
        .await
        .map_err(|e| {
            tracing::warn!("AI parse_text 调用失败: {:?}", e);
            match e {
            AiError::NoApiKey => (
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "未配置 AI API Key，请到设置页配置或联系管理员"})),
            ),
            AiError::Upstream(status, msg) => {
                let code = if status == 429 {
                    StatusCode::TOO_MANY_REQUESTS
                } else {
                    StatusCode::BAD_GATEWAY
                };
                let short_msg = if msg.chars().count() > 500 {
                    format!("{}...", msg.chars().take(500).collect::<String>())
                } else {
                    msg
                };
                (code, Json(json!({"error": format!("AI 服务调用失败: {short_msg}")})))
            }
            AiError::Timeout => (
                StatusCode::GATEWAY_TIMEOUT,
                Json(json!({"error": "AI 服务响应超时（60s）"})),
            ),
        }
        })?;

    // 两阶段清洗 + 反序列化
    let mut parsed: ParsedQuestion = clean_and_parse(&raw_json).map_err(|e| {
        tracing::warn!("clean_and_parse 失败: {e}");
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": format!("AI 返回格式损坏: {e}")})),
        )
    })?;

    // 校验 question_type 合法
    if !["choice", "fill", "solution"].contains(&parsed.question_type.as_str()) {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": format!("未知题型: {}", parsed.question_type)})),
        ));
    }

    // 校验 analysis 至少 1 项
    if parsed.analysis.is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": "AI 返回的解析数组为空"})),
        ));
    }

    // 知识点模糊匹配（填充 kp_matches 供前端自动选中 / 手动确认）
    if !parsed.knowledge_points.is_empty() {
        let tree = crate::handlers::knowledge_points::fetch_tree(&state.pool, None).await;
        if !tree.is_empty() {
            let matches = match_knowledge_points(&parsed.knowledge_points, &tree);
            for m in &matches {
                if m.score < 0.95 && m.matched_name.is_some() {
                    parsed.warnings.push(format!(
                        "知识点「{}」模糊匹配到「{}」(相似度 {:.0}%)",
                        m.ai_name,
                        m.matched_name.as_deref().unwrap(),
                        m.score * 100.0
                    ));
                }
            }
            parsed.kp_matches = matches;
        }
    }

    Ok(Json(ParseResponse { data: parsed }))
}

// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 解析 AI 配置：用户个人 Key 优先，否则平台默认
async fn resolve_ai_config(
    auth: &AuthUser,
    state: &AppState,
) -> Result<(String, String, Option<String>, String), String> {
    // 查用户个人配置
    let user_setting = sqlx::query_as::<_, UserAiSetting>(
        "SELECT * FROM user_ai_settings WHERE user_id = $1",
    )
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("数据库查询失败: {e}"))?;

    if let Some(ref setting) = user_setting {
        if let (Some(enc), Some(iv)) = (&setting.api_key_enc, &setting.api_key_iv) {
            // 有用户个人 Key → 解密使用
            if let Some(ref b64_key) = state.ai_config.key_encryption_key {
                let master_key = parse_master_key(b64_key).map_err(|e| e.to_string())?;
                let api_key =
                    decrypt_api_key(enc, iv, &master_key).map_err(|e| format!("解密失败: {e}"))?;

                let provider_name = setting.provider.clone();
                let base_url = get_provider_base_url(&state.ai_config, &provider_name);
                let model = setting.model_text.clone();
                return Ok((api_key, provider_name, model, base_url));
            }
        }
    }

    // 无用户 Key → 用平台默认
    let ai_config = &state.ai_config;
    let (api_key, provider_name, base_url, default_model) = match ai_config.default_provider.as_str()
    {
        "deepseek" => (
            ai_config.deepseek_api_key.clone(),
            "deepseek",
            ai_config.deepseek_base_url.clone(),
            ai_config.default_model_text.clone(),
        ),
        "qwen" => (
            ai_config.qwen_api_key.clone(),
            "qwen",
            ai_config.qwen_base_url.clone(),
            ai_config.default_model_text.clone(),
        ),
        "openai" => (
            ai_config.openai_api_key.clone(),
            "openai",
            ai_config.openai_base_url.clone(),
            ai_config.default_model_text.clone(),
        ),
        _ => (
            ai_config.deepseek_api_key.clone(),
            "deepseek",
            ai_config.deepseek_base_url.clone(),
            ai_config.default_model_text.clone(),
        ),
    };

    let api_key = api_key.ok_or_else(|| "未配置 AI API Key，请到设置页配置或联系管理员".to_string())?;

    // 用户自定义模型覆盖平台默认
    let model = user_setting
        .as_ref()
        .and_then(|s| s.model_text.clone())
        .or(Some(default_model));

    Ok((api_key, provider_name.to_string(), model, base_url))
}

/// 根据 provider 名称获取 base_url
fn get_provider_base_url(ai_config: &crate::config::AiConfig, provider: &str) -> String {
    match provider {
        "deepseek" => ai_config.deepseek_base_url.clone(),
        "qwen" => ai_config.qwen_base_url.clone(),
        "openai" => ai_config.openai_base_url.clone(),
        _ => ai_config.deepseek_base_url.clone(),
    }
}
