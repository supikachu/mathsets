use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::ai::cleaner::clean_and_parse;
use crate::ai::provider::AiError;
use crate::ai::types::{AnalysisMethod, ParsedQuestion};
use crate::auth::middleware::AuthUser;
use crate::handlers::ai_tagging::match_knowledge_nodes;
use crate::models::ai_setting::{
    decrypt_api_key, encrypt_api_key, parse_master_key, AiSettingsResponse, UpdateAiSettingsRequest,
    UserAiSetting,
};
use crate::AppState;

// ---------------------------------------------------------------------------
// 请求/响应类型
// ---------------------------------------------------------------------------

/// AI 模型类型 — 决定 resolve_ai_config 返回 text 还是 vision 模型配置
#[derive(Clone, Copy)]
pub(crate) enum ModelKind {
    Text,
    Vision,
}

// ---------------------------------------------------------------------------
// 共享后处理管线（parse_text / parse_image 复用）
// ---------------------------------------------------------------------------

/// 将 AiError 映射为 HTTP 错误响应（从 parse_text handler 提取复用）
pub(crate) fn map_ai_error(e: AiError) -> (StatusCode, Json<serde_json::Value>) {
    tracing::warn!("AI 调用失败: {:?}", e);
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
            Json(json!({"error": "AI 服务响应超时（120s），请稍后重试或使用更小的图片"})),
        ),
    }
}


/// 批量后处理：清洗 → 截断检测(补丁七) → 逐题隔离解析(补丁十防连坐) → 知识点匹配
pub(crate) async fn post_process_batch(
    raw_json: &str,
    pool: &sqlx::PgPool,
) -> Result<Vec<ParsedQuestion>, (StatusCode, Json<serde_json::Value>)> {
    // ⚠️ 补丁七：先尝试整体反序列化为 serde_json::Value
    let batch_val: serde_json::Value = match clean_and_parse::<serde_json::Value>(raw_json) {
        Ok(v) => v,
        Err(e) => {
            let err_str = e.to_string();
            tracing::warn!("batch clean_and_parse 失败: {e}");

            // 检测截断特征
            let is_truncated = err_str.contains("EOF")
                || err_str.contains("expected")
                || raw_json.trim_end().ends_with(',')
                || raw_json.trim_end().ends_with('{')
                || (raw_json.matches('{').count() != raw_json.matches('}').count());

            if is_truncated {
                return Err((
                    StatusCode::UNPROCESSABLE_ENTITY,
                    Json(json!({
                        "error": "该页题目过密，AI 识别已达上限。请尝试将页面裁切成两张图片后分别上传。",
                        "code": "ERR_LLM_TRUNCATED"
                    })),
                ));
            }
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": format!("AI 返回格式损坏: {e}"),
                    "code": "ERR_PARSE_FAILED"
                })),
            ));
        }
    };

    // ⚠️ 补丁十：JSON 反序列化防连坐 — 先提取 Value 数组，再逐题 from_value 隔离
    let questions_val = batch_val
        .get("questions")
        .and_then(|q| q.as_array())
        .ok_or_else(|| {
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                Json(json!({
                    "error": "AI 返回格式异常：缺少 questions 数组",
                    "code": "ERR_PARSE_FAILED"
                })),
            )
        })?;

    let mut results = Vec::new();

    for (i, q_val) in questions_val.iter().enumerate() {
        match serde_json::from_value::<ParsedQuestion>(q_val.clone()) {
            Ok(mut q) => {
                // 校验 question_type
                if !["choice", "fill", "solution", "multiple"].contains(&q.question_type.as_str()) {
                    tracing::warn!("第 {} 题题型无效: {}，跳过", i + 1, q.question_type);
                    continue;
                }
                // 校验 analysis
                if q.analysis.is_empty() {
                    q.analysis = vec![AnalysisMethod {
                        title: "解法一".into(),
                        content: "".into(),
                    }];
                    q.warnings.push("AI 返回解析为空，请手动补充".into());
                }
                // 知识点匹配（B3 重构：SQL 三级匹配，失败不影响整体解析）
                if !q.knowledge_points.is_empty() {
                    match match_knowledge_nodes(pool, &q.knowledge_points, None).await {
                        Ok((matched, _)) => {
                            for m in &matched {
                                if m.score < 0.95 {
                                    q.warnings.push(format!(
                                        "知识点「{}」模糊匹配到「{}」(相似度 {:.0}%)",
                                        m.ai_name, m.node_name, m.score * 100.0
                                    ));
                                }
                            }
                            q.kp_matches = matched
                                .iter()
                                .map(|m| crate::ai::kp_matcher::KpMatch {
                                    ai_name: m.ai_name.clone(),
                                    matched_id: Some(m.node_id),
                                    matched_name: Some(m.node_name.clone()),
                                    score: m.score,
                                })
                                .collect();
                        }
                        Err(e) => {
                            tracing::warn!(
                                "第 {} 题知识点匹配失败（不影响解析）: {:?}",
                                i + 1,
                                e.1
                            );
                        }
                    }
                }
                results.push(q);
            }
            Err(e) => {
                // ⚠️ 补丁十核心：单题解析失败只记录 warning，不影响同批次其他题目
                tracing::warn!("第 {} 题解析失败，跳过（不影响其他题目）: {}", i + 1, e);
            }
        }
    }

    if results.is_empty() {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error": "AI 未识别出有效题目", "code": "ERR_NO_VALID_QUESTIONS"})),
        ));
    }

    Ok(results)
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


// ---------------------------------------------------------------------------
// 辅助函数
// ---------------------------------------------------------------------------

/// 解析 AI 配置：用户个人 Key 优先，否则平台默认
/// model_kind 决定返回 text 还是 vision 模型
pub(crate) async fn resolve_ai_config(
    auth: &AuthUser,
    state: &AppState,
    model_kind: ModelKind,
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
                let model = match model_kind {
                    ModelKind::Text => setting.model_text.clone(),
                    ModelKind::Vision => setting.model_vision.clone(),
                };
                return Ok((api_key, provider_name, model, base_url));
            }
        }
    }

    // 无用户 Key → 用平台默认
    let ai_config = &state.ai_config;
    let default_model = match model_kind {
        ModelKind::Text => ai_config.default_model_text.clone(),
        ModelKind::Vision => ai_config.default_model_vision.clone(),
    };

    // ⚠️ 智能路由：Vision 模式下根据模型名前缀选择正确的 provider
    // 避免用 deepseek provider 发送 qwen-vl-plus 等视觉模型请求导致调用失败
    let preferred_provider = match model_kind {
        ModelKind::Text => ai_config.default_provider.clone(),
        ModelKind::Vision => match default_model.as_str() {
            m if m.starts_with("qwen") => "qwen".to_string(),
            m if m.starts_with("gpt") => "openai".to_string(),
            m if m.starts_with("deepseek") => "deepseek".to_string(),
            _ => ai_config.default_provider.clone(),
        },
    };

    let (api_key, provider_name, base_url) = match preferred_provider.as_str() {
        "deepseek" => (
            ai_config.deepseek_api_key.clone(),
            "deepseek",
            ai_config.deepseek_base_url.clone(),
        ),
        "qwen" => (
            ai_config.qwen_api_key.clone(),
            "qwen",
            ai_config.qwen_base_url.clone(),
        ),
        "openai" => (
            ai_config.openai_api_key.clone(),
            "openai",
            ai_config.openai_base_url.clone(),
        ),
        _ => (
            ai_config.deepseek_api_key.clone(),
            "deepseek",
            ai_config.deepseek_base_url.clone(),
        ),
    };

    let api_key = api_key.ok_or_else(|| {
        format!(
            "未配置 {} 的 API Key（视觉模型 {} 需要对应的 API Key），请在 .env 中设置或到设置页配置",
            provider_name, default_model
        )
    })?;

    // 用户自定义模型覆盖平台默认
    let model = match model_kind {
        ModelKind::Text => user_setting.as_ref().and_then(|s| s.model_text.clone()),
        ModelKind::Vision => user_setting.as_ref().and_then(|s| s.model_vision.clone()),
    }
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
