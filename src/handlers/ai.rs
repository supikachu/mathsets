use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::LazyLock;

use crate::ai::cleaner::{clean_and_parse, repair_truncated_batch};
use crate::ai::ocr::OcrConfig;
use crate::ai::provider::AiError;
use crate::ai::types::{AnalysisMethod, ParsedAnswer, ParsedQuestion};
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

// ---------------------------------------------------------------------------
// 第二道防线：题干选项残留正则剥离
// ---------------------------------------------------------------------------

/// 匹配题干末尾的选项残留块（A→B→C→D 顺序完整且延伸到字符串结尾）
///
/// 设计要点（保守优先，避免误删真实题干）：
/// - 必须匹配到完整的 A、B、C、D 四个选项前缀（缺一不匹配，避免误伤零散 A/B）
/// - 选项前缀形如 `A.` `A、` `A)`，前缀前需为行首 / 句末标点（。；;！？），
///   避免误伤正文里的 "点 A."、"线段 AB."、"已知 A(1,2)" 等
/// - lazy `.*?` + `$` 锚定到字符串结尾，只剥离尾部完整选项块
/// - `(?is)`：i 忽略大小写，s 让 `.` 跨换行（选项常跨多行）
static OPTIONS_RESIDUE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)(?:^|\n|[。；;！？])\s*A[\.、\)]\s*.*?B[\.、\)]\s*.*?C[\.、\)]\s*.*?D[\.、\)]\s*.*$",
    )
    .expect("选项残留正则编译失败")
});

/// 第二道防线：正则剥离选择题题干末尾的选项残留
///
/// 大模型偶尔不听 Prompt 指令，会把 "A. xxx B. xxx C. xxx D. xxx" 残留在 stem 里，
/// 导致前端题干区与选项区重复渲染。本函数在结构化解析后做兜底清洗。
///
/// - 仅对 choice / multiple 题型生效
/// - 仅当 `options` 数组已填充时才剥离（否则剥离会丢失唯一选项副本，宁可不剥）
/// - 命中时剥离残留并追加 warning 供审计
fn strip_options_residue_from_stem(q: &mut ParsedQuestion) {
    if !matches!(q.question_type.as_str(), "choice" | "multiple") {
        return;
    }
    // 仅当 options 已填充时剥离——否则剥离会丢失唯一选项副本，宁可不剥
    let has_options = q.options.as_ref().is_some_and(|o| !o.is_empty());
    if !has_options {
        return;
    }
    if let Some(m) = OPTIONS_RESIDUE_RE.find(&q.stem) {
        let new_stem = q.stem[..m.start()].trim_end().to_string();
        if new_stem != q.stem {
            tracing::info!(
                "剥离题干选项残留：{} 字符 → {} 字符",
                q.stem.chars().count(),
                new_stem.chars().count()
            );
            q.warnings.push("已自动剥离题干中残留的选项文本".into());
            q.stem = new_stem;
        }
    }
}

/// 选择题题干末尾用于填涂答案的空括号 → `$(\hspace{2em})$`
///
/// OCR 常把「的集合是 ()」写成裸括号，预览里几乎看不见空位。
/// 只替换「最后一处、后面只剩空白或配图」的空括号，避免误伤：
/// - 函数 `f()`、区间 `(0,1)`、题号 `(1)(2)`
/// - 已写成 `$(\hspace{2em})$` 的括号
/// - `$...$` 公式内部
static CHOICE_EMPTY_PARENS_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"[（(][\s\u{00a0}\u{3000}]*[）)]").expect("作答空括号正则编译失败")
});

static STEM_TRAILING_IMG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)^(?:\s|!\[[^\]]*\]\([^)]*\))*$").expect("题干尾部配图正则编译失败")
});

fn dollar_math_spans(stem: &str) -> Vec<(usize, usize)> {
    let bytes = stem.as_bytes();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' {
            let start = i;
            i += 1;
            while i < bytes.len() && bytes[i] != b'$' {
                i += 1;
            }
            if i < bytes.len() {
                spans.push((start, i + 1));
                i += 1;
            }
        } else {
            i += 1;
        }
    }
    spans
}

fn index_in_math(spans: &[(usize, usize)], idx: usize) -> bool {
    spans.iter().any(|&(a, b)| idx >= a && idx < b)
}

fn normalize_choice_answer_blank(q: &mut ParsedQuestion) {
    if !matches!(q.question_type.as_str(), "choice" | "multiple") {
        return;
    }
    let math = dollar_math_spans(&q.stem);
    let mut last: Option<(usize, usize)> = None;
    for m in CHOICE_EMPTY_PARENS_RE.find_iter(&q.stem) {
        if index_in_math(&math, m.start()) {
            continue;
        }
        if m.start() > 0 {
            if let Some(ch) = q.stem[..m.start()].chars().last() {
                if ch.is_ascii_alphanumeric() || ch == '_' || ch == '\\' {
                    continue;
                }
            }
        }
        last = Some((m.start(), m.end()));
    }
    let Some((start, end)) = last else { return };
    if !STEM_TRAILING_IMG_RE.is_match(&q.stem[end..]) {
        return;
    }
    let mut new_stem = String::with_capacity(q.stem.len() + 16);
    new_stem.push_str(&q.stem[..start]);
    new_stem.push_str("$(\\hspace{2em})$");
    new_stem.push_str(&q.stem[end..]);
    q.stem = new_stem;
}

/// solution_methods 字符串数组 → 对象数组归一化（纯函数）
///
/// LLM 对 `"solution_methods": [{"name":"...","confidence":...}]` 的遵循不稳定，
/// 常退化为 `["数形结合"]` 纯字符串数组——直接反序列化 `Vec<SolutionMethod>`
/// 会类型不匹配导致整题被丢弃。此处把字符串元素包成 `{name}` 对象。
fn normalize_solution_methods(mut q_val: serde_json::Value) -> serde_json::Value {
    let Some(arr) = q_val.get("solution_methods").and_then(|v| v.as_array()) else {
        return q_val;
    };
    let needs_fix = arr.iter().any(|e| e.is_string());
    if !needs_fix {
        return q_val;
    }
    let normalized: Vec<serde_json::Value> = arr
        .iter()
        .map(|e| match e.as_str() {
            Some(name) => serde_json::json!({ "name": name }),
            None => e.clone(),
        })
        .collect();
    q_val["solution_methods"] = serde_json::Value::Array(normalized);
    q_val
}

/// 批量后处理：清洗 → 截断检测(补丁七) → 逐题隔离解析(补丁十防连坐) → 知识点匹配
///
/// v1.1（T1.12）：当 Stage 2 输出被 `max_tokens` 截断导致 JSON 残缺时，
/// 调用 `repair_truncated_batch` 丢弃末题、补全闭合符，返回已成功解析的前 N-1 题，
/// 并在每题 warnings 标注截断提示（AC-12：不整体失败）。
///
/// M4：可见性改为 `pub(crate)` 以供 worker 调用（异步任务复用同一后处理管线）。
pub(crate) async fn post_process_batch(
    raw_json: &str,
    pool: &sqlx::PgPool,
) -> Result<Vec<ParsedQuestion>, (StatusCode, Json<serde_json::Value>)> {
    // ⚠️ 补丁七：先尝试整体反序列化为 serde_json::Value
    let (batch_val, truncated): (serde_json::Value, bool) = match clean_and_parse::<serde_json::Value>(raw_json) {
        Ok(v) => (v, false),
        Err(e) => {
            let err_str = e.to_string();
            tracing::warn!("batch clean_and_parse 失败: {e}");

            // v1.1（T1.12）：截断容错 — 尝试修复后重新解析
            if let Some(repaired) = repair_truncated_batch(raw_json) {
                match clean_and_parse::<serde_json::Value>(&repaired) {
                    Ok(v) => {
                        tracing::warn!(
                            "Stage 2 输出疑似被 max_tokens 截断，已自动修复（丢弃末题，保留前 N-1 题）"
                        );
                        (v, true)
                    }
                    Err(re) => {
                        tracing::warn!("截断修复后仍解析失败: {re}");
                        return truncation_or_parse_error(&err_str, raw_json);
                    }
                }
            } else {
                return truncation_or_parse_error(&err_str, raw_json);
            }
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

    // 截断修复成功时，给每题追加提示，提醒用户核对结果
    let truncate_warning: Option<&str> = if truncated {
        Some("本次识别因内容过长被截断，已自动丢弃末题并保留前 N 题，请核对结果")
    } else {
        None
    };

    let mut results = Vec::new();

    for (i, q_val) in questions_val.iter().enumerate() {
        // solution_methods 容错归一化：LLM 常输出 ["数形结合"] 字符串数组而非
        // [{"name":"..."}] 对象数组 → 先转对象再反序列化，避免整题因类型不匹配被丢弃
        let q_val = normalize_solution_methods(q_val.clone());
        match serde_json::from_value::<ParsedQuestion>(q_val) {
            Ok(mut q) => {
                // 校验 question_type
                if !["choice", "fill", "solution", "multiple"].contains(&q.question_type.as_str()) {
                    tracing::warn!("第 {} 题题型无效: {}，跳过", i + 1, q.question_type);
                    continue;
                }
                // 第二道防线：剥离选择题题干末尾的选项残留
                strip_options_residue_from_stem(&mut q);
                // 选择题作答空括号 () / （） → $(\hspace{2em})$
                normalize_choice_answer_blank(&mut q);
                // :::img-row 围栏闭合清洗：防 token 截断导致缺 ::: 闭合标记
                q.sanitize_img_row_fences();
                // 校验 analysis
                if q.analysis.is_empty() {
                    q.analysis = vec![AnalysisMethod {
                        title: "解法一".into(),
                        content: "".into(),
                    }];
                    q.warnings.push("AI 返回解析为空，请手动补充".into());
                }
                // v1.2：correct_answer 为 None（LLM 输出了 null）→ 按题型补空默认值
                if q.correct_answer.is_none() {
                    q.correct_answer = Some(ParsedAnswer::empty_for_type(&q.question_type));
                    q.warnings.push("AI 未返回答案，已自动填充空答案".into());
                }
                // 知识点匹配（限定 knowledge 树，杜绝跨树错配；失败不影响整体解析）
                if !q.knowledge_points.is_empty() {
                    match match_knowledge_nodes(pool, &q.knowledge_points, None, "knowledge").await {
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
                            crate::ai::tagging::shadow::maybe_log_knowledge_shadow(
                                pool,
                                &q.knowledge_points,
                            )
                            .await;
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
                // v1.1（T1.12）：截断修复成功时，每题标注提示
                if let Some(w) = truncate_warning {
                    q.warnings.push(w.to_string());
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

/// 截断检测 + 错误归类（post_process_batch 兜底）
///
/// 当 `clean_and_parse` 与 `repair_truncated_batch` 均失败时调用。
/// 判断是否为 `max_tokens` 截断导致的解析失败，返回对应的错误码与提示。
fn truncation_or_parse_error(
    err_str: &str,
    raw_json: &str,
) -> Result<Vec<ParsedQuestion>, (StatusCode, Json<serde_json::Value>)> {
    let is_truncated = err_str.contains("EOF")
        || err_str.contains("expected")
        || raw_json.trim_end().ends_with(',')
        || raw_json.trim_end().ends_with('{')
        || raw_json.matches('{').count() != raw_json.matches('}').count();

    if is_truncated {
        Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "该页题目过密，AI 识别已达上限。请尝试将页面裁切成两张图片后分别上传。",
                "code": "ERR_LLM_TRUNCATED"
            })),
        ))
    } else {
        Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": format!("AI 返回格式损坏: {err_str}"),
                "code": "ERR_PARSE_FAILED"
            })),
        ))
    }
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
            ocr_provider: s.ocr_provider,
            has_doc2x_key: s.doc2x_api_key_enc.is_some(),
            mineru_endpoint: s.mineru_api_endpoint,
            has_mineru_key: s.mineru_api_key_enc.is_some(),
        },
        None => AiSettingsResponse {
            provider: "deepseek".to_string(),
            has_api_key: false,
            model_text: None,
            model_vision: None,
            ocr_provider: "auto".to_string(),
            has_doc2x_key: false,
            mineru_endpoint: None,
            has_mineru_key: false,
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

    // M3：加密 Doc2X API Key（若提供）
    let (doc2x_enc, doc2x_iv) = match &req.doc2x_api_key {
        Some(key) if !key.is_empty() => {
            let (enc, iv) = encrypt_api_key(key, &master_key).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("Doc2X Key 加密失败: {e}")})),
                )
            })?;
            (Some(enc), Some(iv))
        }
        _ => (None, None),
    };

    // M3：加密 MinerU API Key（若提供，M4 启用但字段已就绪）
    let (mineru_enc, mineru_iv) = match &req.mineru_api_key {
        Some(key) if !key.is_empty() => {
            let (enc, iv) = encrypt_api_key(key, &master_key).map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": format!("MinerU Key 加密失败: {e}")})),
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
        INSERT INTO user_ai_settings
            (user_id, provider, api_key_enc, api_key_iv, model_text, model_vision,
             ocr_provider, doc2x_api_key_enc, doc2x_api_key_iv,
             mineru_api_endpoint, mineru_api_key_enc, mineru_api_key_iv, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, 'auto'), $8, $9, $10, $11, $12, NOW())
        ON CONFLICT (user_id) DO UPDATE SET
            provider = COALESCE($2, user_ai_settings.provider),
            api_key_enc = CASE WHEN $3 IS NOT NULL THEN $3 ELSE user_ai_settings.api_key_enc END,
            api_key_iv = CASE WHEN $4 IS NOT NULL THEN $4 ELSE user_ai_settings.api_key_iv END,
            model_text = $5,
            model_vision = $6,
            ocr_provider = COALESCE($7, user_ai_settings.ocr_provider),
            doc2x_api_key_enc = CASE WHEN $8 IS NOT NULL THEN $8 ELSE user_ai_settings.doc2x_api_key_enc END,
            doc2x_api_key_iv = CASE WHEN $9 IS NOT NULL THEN $9 ELSE user_ai_settings.doc2x_api_key_iv END,
            mineru_api_endpoint = COALESCE($10, user_ai_settings.mineru_api_endpoint),
            mineru_api_key_enc = CASE WHEN $11 IS NOT NULL THEN $11 ELSE user_ai_settings.mineru_api_key_enc END,
            mineru_api_key_iv = CASE WHEN $12 IS NOT NULL THEN $12 ELSE user_ai_settings.mineru_api_key_iv END,
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
    .bind(&req.ocr_provider)
    .bind(&doc2x_enc)
    .bind(&doc2x_iv)
    .bind(&req.mineru_endpoint)
    .bind(&mineru_enc)
    .bind(&mineru_iv)
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
        ocr_provider: setting.ocr_provider,
        has_doc2x_key: setting.doc2x_api_key_enc.is_some(),
        mineru_endpoint: setting.mineru_api_endpoint,
        has_mineru_key: setting.mineru_api_key_enc.is_some(),
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

/// OCR 引擎决策来源判定（纯函数，供 resolve_ocr_config 决策日志使用）
///
/// 优先级：任务/请求 override > 用户偏好（非 auto）> auto 兜底。
/// 返回值对应 tracing target=ocr::engine_select 日志的 `decision_source` 字段：
/// - `task_override`   — 任务显式指定（ocr_provider_override / API 参数），即使值与偏好相同
/// - `user_preference` — 用户设置页保存的偏好（非 auto，含显式 qwen_vl）
/// - `auto_default`    — 无偏好 / 偏好为 auto → 兜底
pub(crate) fn ocr_decision_source(requested: Option<&str>, user_pref: Option<&str>) -> &'static str {
    match requested {
        Some(_) => "task_override",
        None => match user_pref {
            Some("auto") | None => "auto_default",
            Some(_) => "user_preference",
        },
    }
}

/// 解析 OCR 引擎配置（v1.1，两阶段流水线 Stage 1）
///
/// 优先级（M3 配置下沉）：
/// 1. `requested` 显式参数（multipart 表单 / test-connection 临时覆盖）最高
/// 2. 用户在设置页保存的 `ocr_provider` 偏好（user_ai_settings 表）
/// 3. 默认 `"auto"`
///
/// 引擎选择：
/// - `doc2x`：优先用用户个人 Doc2X Key（DB 解密），否则回退平台默认 Key
/// - `auto` / `qwen_vl` / 其他：复用 `resolve_ai_config` 的 Vision 配置，兜底 qwen_vl（AC-07）
///
/// 注意：`auto` 始终映射为 qwen_vl（不触发降级路径），仅显式 `doc2x` 才走降级逻辑。
pub(crate) async fn resolve_ocr_config(
    auth: &AuthUser,
    state: &AppState,
    requested: Option<&str>,
) -> Result<OcrConfig, String> {
    // 查询用户保存的 OCR 偏好 + Doc2X 个人 Key
    let user_setting = sqlx::query_as::<_, UserAiSetting>(
        "SELECT * FROM user_ai_settings WHERE user_id = $1",
    )
    .bind(auth.id)
    .fetch_optional(&state.pool)
    .await
    .map_err(|e| format!("查询 OCR 配置失败: {e}"))?;

    // 有效引擎：显式 requested > 用户保存偏好 > auto
    let effective = requested
        .map(|s| s.to_string())
        .or_else(|| user_setting.as_ref().map(|s| s.ocr_provider.clone()))
        .unwrap_or_else(|| "auto".to_string());

    // 决策来源：任务/请求 override > 用户偏好（非 auto）> auto 兜底
    let decision_source = ocr_decision_source(
        requested,
        user_setting.as_ref().map(|s| s.ocr_provider.as_str()),
    );
    // 引擎选择决策追踪（target=ocr::engine_select 便于集中检索分析切换原因）
    tracing::info!(
        target: "ocr::engine_select",
        user_id = %auth.id,
        requested = ?requested,
        user_pref = ?user_setting.as_ref().map(|s| s.ocr_provider.as_str()),
        effective = %effective,
        decision_source,
        "OCR 引擎决策"
    );

    if effective == "doc2x" {
        let base_url = state.ai_config.doc2x_base_url.clone();

        // 是否配置了个人 Key 密文（用于识别"配了却没用上"的场景）
        let has_personal_cipher = user_setting
            .as_ref()
            .map(|s| s.doc2x_api_key_enc.is_some() && s.doc2x_api_key_iv.is_some())
            .unwrap_or(false);

        // 优先用用户个人 Doc2X Key（DB 解密），否则回退平台默认 Key
        let personal_key = user_setting.as_ref().and_then(|s| {
            let enc = s.doc2x_api_key_enc.as_ref()?;
            let iv = s.doc2x_api_key_iv.as_ref()?;
            let b64 = state.ai_config.key_encryption_key.as_ref()?;
            parse_master_key(b64)
                .and_then(|mk| decrypt_api_key(enc, iv, &mk))
                .ok()
        });
        if has_personal_cipher && personal_key.is_none() {
            tracing::warn!(
                target: "ocr::engine_select",
                user_id = %auth.id,
                "Doc2X 个人 Key 密文存在但解密失败（或主密钥未配置），本次回退平台 Key"
            );
        }
        let key_source = if personal_key.is_some() { "personal" } else { "platform" };
        let api_key = personal_key
            .or_else(|| state.ai_config.doc2x_api_key.clone())
            .ok_or_else(|| {
                "未配置 Doc2X API Key，请在设置页配置或切换其他 OCR 引擎".to_string()
            })?;
        tracing::info!(
            target: "ocr::engine_select",
            user_id = %auth.id,
            engine = "doc2x",
            key_source,
            "OCR 引擎决策结果"
        );

        return Ok(OcrConfig {
            provider: "doc2x".into(),
            api_key,
            base_url,
            model: None,
            upload_dir: Some(state.upload_dir.clone()),
        });
    }

    if effective == "mineru_local" || effective == "mineru_api" {
        // M4：MinerU 纯用户私有部署，平台无默认端点
        let endpoint = user_setting
            .as_ref()
            .and_then(|s| s.mineru_api_endpoint.clone())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                "未配置 MinerU 服务端点，请在设置页填写私有部署地址（如 http://127.0.0.1:8000）"
                    .to_string()
            })?;

        // API Key 可选（私有部署免鉴权场景为空）
        let api_key = user_setting
            .as_ref()
            .and_then(|s| {
                let enc = s.mineru_api_key_enc.as_ref()?;
                let iv = s.mineru_api_key_iv.as_ref()?;
                let b64 = state.ai_config.key_encryption_key.as_ref()?;
                parse_master_key(b64)
                    .and_then(|mk| decrypt_api_key(enc, iv, &mk))
                    .ok()
            })
            .unwrap_or_default();

        tracing::info!(
            target: "ocr::engine_select",
            user_id = %auth.id,
            engine = %effective,
            endpoint_source = "user_private",
            "OCR 引擎决策结果"
        );
        return Ok(OcrConfig {
            provider: "mineru_local".into(),
            api_key,
            base_url: endpoint,
            model: None,
            upload_dir: Some(state.upload_dir.clone()),
        });
    }

    // auto / qwen_vl / 未知引擎 → 兜底 qwen_vl（保持 AC-07 等价行为）
    if effective != "auto" && effective != "qwen_vl" {
        tracing::warn!(
            target: "ocr::engine_select",
            user_id = %auth.id,
            effective = %effective,
            "OCR 引擎 `{effective}` 在当前版本未实现，自动降级 qwen_vl 兜底"
        );
    }

    let (api_key, _provider_name, model, base_url) =
        resolve_ai_config(auth, state, ModelKind::Vision).await?;

    tracing::info!(
        target: "ocr::engine_select",
        user_id = %auth.id,
        engine = "qwen_vl",
        reason = if effective == "auto" { "auto_default_vision_model" } else { "qwen_vl_direct" },
        "OCR 引擎决策结果"
    );

    Ok(OcrConfig {
        provider: "qwen_vl".into(),
        api_key,
        base_url,
        model,
        upload_dir: Some(state.upload_dir.clone()),
    })
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

// ---------------------------------------------------------------------------
// OCR 引擎连接测试（M2 新增）
// ---------------------------------------------------------------------------

/// 连接测试请求：前端在保存 OCR 配置前先探测 Key/Endpoint 是否可用
#[derive(Deserialize)]
pub struct TestOcrConnectionRequest {
    /// 引擎名：doc2x | qwen_vl | auto
    pub provider: String,
    /// 可选自定义 API Key（前端临时输入，未填则用平台默认）
    pub api_key: Option<String>,
    /// 可选自定义 endpoint（前端临时输入，未填则用平台默认）
    pub endpoint: Option<String>,
}

/// 连接测试响应
#[derive(Serialize)]
pub struct TestOcrConnectionResponse {
    pub ok: bool,
    pub latency_ms: u64,
    pub message: String,
}

/// 测试 OCR 引擎连接 — 轻量探测，不消耗配额
///
/// - `doc2x`：GET `/api/v2/parse/status?uid=test-connection-probe`（伪造 uid），
///   401/403 → Key 无效；其他 4xx/2xx → Key 有效（auth 通过即可）
/// - `qwen_vl` / `auto`：GET `/v1/models`（OpenAI 兼容鉴权探测），
///   200 → ok；401/403 → Key 无效；其他 → 探测失败
pub async fn test_ocr_connection(
    Extension(auth): Extension<AuthUser>,
    State(state): State<AppState>,
    Json(req): Json<TestOcrConnectionRequest>,
) -> Result<Json<TestOcrConnectionResponse>, (StatusCode, Json<serde_json::Value>)> {
    use std::time::Instant;

    let start = Instant::now();
    let provider = req.provider.trim().to_lowercase();

    // 构造短超时 client（探测请求 10s 足矣，避免前端长时间等待）
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .no_proxy()
        .build()
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("创建 HTTP 客户端失败: {e}")})),
            )
        })?;

    let (ok, message) = match provider.as_str() {
        "doc2x" => {
            let base_url = req
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches('/').to_string())
                .unwrap_or_else(|| state.ai_config.doc2x_base_url.clone());

            // Key 优先级：前端临时输入 > 用户个人 Key（DB 解密） > 平台默认
            let api_key = match req.api_key.as_deref().filter(|s| !s.is_empty()) {
                Some(k) => k.to_string(),
                None => {
                    // 查用户个人 Doc2X Key（解密），否则回退平台默认
                    let personal = sqlx::query_as::<_, UserAiSetting>(
                        "SELECT * FROM user_ai_settings WHERE user_id = $1",
                    )
                    .bind(auth.id)
                    .fetch_optional(&state.pool)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|s| {
                        let enc = s.doc2x_api_key_enc?;
                        let iv = s.doc2x_api_key_iv?;
                        let b64 = state.ai_config.key_encryption_key.as_ref()?;
                        parse_master_key(b64)
                            .and_then(|mk| decrypt_api_key(&enc, &iv, &mk))
                            .ok()
                    });
                    match personal.or_else(|| state.ai_config.doc2x_api_key.clone()) {
                        Some(k) => k,
                        None => {
                            return Ok(Json(TestOcrConnectionResponse {
                                ok: false,
                                latency_ms: start.elapsed().as_millis() as u64,
                                message: "未配置 Doc2X API Key".to_string(),
                            }));
                        }
                    }
                }
            };

            probe_doc2x(&client, &api_key, &base_url).await
        }
        "mineru_local" | "mineru_api" => {
            // M4：MinerU 私有部署探测
            // 端点优先级：前端临时输入 > 用户保存的 endpoint > 默认（空，必填）
            let endpoint = if let Some(e) = req
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches('/').to_string())
            {
                Some(e)
            } else {
                // 查用户保存的 MinerU endpoint
                sqlx::query_as::<_, UserAiSetting>(
                    "SELECT * FROM user_ai_settings WHERE user_id = $1",
                )
                .bind(auth.id)
                .fetch_optional(&state.pool)
                .await
                .ok()
                .flatten()
                .and_then(|s| s.mineru_api_endpoint)
            };
            let endpoint = match endpoint {
                Some(e) => e,
                None => {
                    return Ok(Json(TestOcrConnectionResponse {
                        ok: false,
                        latency_ms: start.elapsed().as_millis() as u64,
                        message: "未配置 MinerU 服务端点".to_string(),
                    }));
                }
            };

            // Key 优先级：前端临时输入 > 用户个人 Key（DB 解密）
            let api_key = match req.api_key.as_deref().filter(|s| !s.is_empty()) {
                Some(k) => Some(k.to_string()),
                None => {
                    let personal = sqlx::query_as::<_, UserAiSetting>(
                        "SELECT * FROM user_ai_settings WHERE user_id = $1",
                    )
                    .bind(auth.id)
                    .fetch_optional(&state.pool)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|s| {
                        let enc = s.mineru_api_key_enc?;
                        let iv = s.mineru_api_key_iv?;
                        let b64 = state.ai_config.key_encryption_key.as_ref()?;
                        parse_master_key(b64)
                            .and_then(|mk| decrypt_api_key(&enc, &iv, &mk))
                            .ok()
                    });
                    personal
                }
            };

            probe_mineru(&client, api_key.as_deref(), &endpoint).await
        }
        "qwen_vl" | "auto" => {
            // 复用 Vision 配置（用户 Key 优先）
            let cfg = match resolve_ocr_config(&auth, &state, Some("qwen_vl")).await {
                Ok(c) => c,
                Err(e) => {
                    return Ok(Json(TestOcrConnectionResponse {
                        ok: false,
                        latency_ms: start.elapsed().as_millis() as u64,
                        message: e,
                    }));
                }
            };
            // 前端临时输入覆盖
            let api_key = req
                .api_key
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .unwrap_or(cfg.api_key);
            let base_url = req
                .endpoint
                .as_deref()
                .filter(|s| !s.is_empty())
                .map(|s| s.trim_end_matches('/').to_string())
                .unwrap_or(cfg.base_url);

            probe_openai_compatible(&client, &api_key, &base_url).await
        }
        other => {
            return Ok(Json(TestOcrConnectionResponse {
                ok: false,
                latency_ms: start.elapsed().as_millis() as u64,
                message: format!("不支持的 OCR 引擎: {other}"),
            }));
        }
    };

    Ok(Json(TestOcrConnectionResponse {
        ok,
        latency_ms: start.elapsed().as_millis() as u64,
        message,
    }))
}

/// 探测 Doc2X 鉴权：GET /api/v2/parse/status?uid=test-connection-probe
///
/// - 401/403 → Key 无效
/// - 2xx / 其他 4xx（如 400 业务参数错）→ auth 通过，Key 有效
/// - 超时 / 网络错误 → 探测失败
async fn probe_doc2x(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
) -> (bool, String) {
    let url = format!("{}/api/v2/parse/status?uid=test-connection-probe", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await;

    match resp {
        Ok(r) => {
            let status = r.status().as_u16();
            if status == 401 || status == 403 {
                (false, "API Key 无效或权限不足".to_string())
            } else if r.status().is_success() {
                (true, "连接成功".to_string())
            } else {
                // 4xx 业务错误（如 uid 不存在）说明 auth 通过
                (true, format!("连接成功（HTTP {status}）"))
            }
        }
        Err(e) if e.is_timeout() => (false, "请求超时".to_string()),
        Err(e) => (false, format!("网络错误: {e}")),
    }
}

/// 探测 OpenAI 兼容鉴权：GET /v1/models
///
/// - 200 → ok
/// - 401/403 → Key 无效
/// - 其他 → 探测失败
async fn probe_openai_compatible(
    client: &reqwest::Client,
    api_key: &str,
    base_url: &str,
) -> (bool, String) {
    let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .send()
        .await;

    match resp {
        Ok(r) => {
            let status = r.status().as_u16();
            if r.status().is_success() {
                (true, "连接成功".to_string())
            } else if status == 401 || status == 403 {
                (false, "API Key 无效".to_string())
            } else {
                (false, format!("服务返回 HTTP {status}"))
            }
        }
        Err(e) if e.is_timeout() => (false, "请求超时".to_string()),
        Err(e) => (false, format!("网络错误: {e}")),
    }
}

/// 探测 MinerU 服务连接与鉴权
///
/// 根据端点是否为官方云端 API（含 `mineru.net`）采用不同策略：
///
/// ### 云端模式（`mineru.net`）
/// 向 Precision API `POST /v4/file-urls/batch` 发送空文件请求，校验 Bearer Token：
/// - 401/403 → "API Key 无效"
/// - 200 → "MinerU 官方 API 鉴权成功"
/// - 400/其他 4xx（非 401/403）→ 鉴权通过（业务参数错误不影响 Key 有效性）
///
/// ### 私有部署模式
/// `GET /docs`（FastAPI Swagger）探测服务可达性：
/// - 200 → 服务可达
/// - 401/403 → 鉴权失败
/// - 404 → 回退 `/file_parse` 探测（POST 空体，期望 4xx 非 401 表示服务在线）
/// - 其他/网络错误 → 探测失败
async fn probe_mineru(
    client: &reqwest::Client,
    api_key: Option<&str>,
    base_url: &str,
) -> (bool, String) {
    let base = base_url.trim_end_matches('/');

    // 云端模式：用真实鉴权接口校验 API Key
    if base.to_lowercase().contains("mineru.net") {
        return probe_mineru_cloud(client, api_key, base).await;
    }

    // 私有部署模式：探测服务可达性
    probe_mineru_private(client, api_key, base).await
}

/// 云端探测：POST /v4/file-urls/batch 校验 Bearer Token 合法性
///
/// 使用空 files 数组避免创建真实任务、不消耗配额。
/// - 401/403 → API Key 无效
/// - 200/201 → 鉴权成功
/// - 400 等非 401/403 的 4xx → 鉴权通过（业务参数错误不影响 Key 有效性）
async fn probe_mineru_cloud(
    client: &reqwest::Client,
    api_key: Option<&str>,
    base_url: &str,
) -> (bool, String) {
    let url = format!("{base_url}/v4/file-urls/batch");

    // 空 files 数组：服务端会返回 400（参数错误），但不创建任务、不消耗配额
    let body = serde_json::json!({
        "files": [],
        "model_version": "vlm"
    });

    let mut req = client.post(&url).json(&body);
    if let Some(k) = api_key {
        req = req.header("Authorization", format!("Bearer {k}"));
    }

    match req.send().await {
        Ok(r) => {
            let status = r.status().as_u16();
            if status == 401 || status == 403 {
                (false, "API Key 无效".to_string())
            } else if status == 200 || status == 201 {
                (true, "MinerU 官方 API 鉴权成功".to_string())
            } else if r.status().is_success() {
                (true, "MinerU 官方 API 鉴权成功".to_string())
            } else {
                // 400 等非 401/403 的 4xx → 鉴权通过（业务参数错误）
                (true, "MinerU 官方 API 鉴权成功".to_string())
            }
        }
        Err(e) if e.is_timeout() => (false, "请求超时".to_string()),
        Err(e) => (false, format!("网络错误: {e}")),
    }
}

/// 私有部署探测：GET /docs 检查服务可达性
async fn probe_mineru_private(
    client: &reqwest::Client,
    api_key: Option<&str>,
    base_url: &str,
) -> (bool, String) {
    let docs_url = format!("{base_url}/docs");

    let mut req = client.get(&docs_url);
    if let Some(k) = api_key {
        req = req.header("Authorization", format!("Bearer {k}"));
    }

    match req.send().await {
        Ok(r) => {
            let status = r.status().as_u16();
            if r.status().is_success() {
                (true, "连接成功".to_string())
            } else if status == 401 || status == 403 {
                (false, "API Key 无效或权限不足".to_string())
            } else if status == 404 {
                // /docs 未开放，回退探测 /file_parse（POST 空体，期望 4xx 非 401 表示服务在线）
                let parse_url = format!("{base_url}/file_parse");
                let mut req2 = client.post(&parse_url);
                if let Some(k) = api_key {
                    req2 = req2.header("Authorization", format!("Bearer {k}"));
                }
                match req2.send().await {
                    Ok(r2) if r2.status().is_success() => (true, "连接成功".to_string()),
                    Ok(r2) if r2.status().as_u16() == 401 || r2.status().as_u16() == 403 => {
                        (false, "API Key 无效或权限不足".to_string())
                    }
                    Ok(_) => {
                        // 400/415 等 4xx → 服务在线，鉴权通过
                        (true, "连接成功".to_string())
                    }
                    Err(e) if e.is_timeout() => (false, "请求超时".to_string()),
                    Err(e) => (false, format!("网络错误: {e}")),
                }
            } else {
                (true, format!("服务可达（HTTP {status}）"))
            }
        }
        Err(e) if e.is_timeout() => (false, "请求超时".to_string()),
        Err(e) => (false, format!("网络错误: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::types::ParsedOption;

    /// 构造一个 choice 题型的 ParsedQuestion 用于测试
    fn make_choice(stem: &str, options_filled: bool) -> ParsedQuestion {
        ParsedQuestion {
            question_type: "choice".into(),
            sub_type: None,
            difficulty: None,
            stem: stem.into(),
            options: if options_filled {
                Some(vec![
                    ParsedOption { label: "A".into(), content: "1".into() },
                    ParsedOption { label: "B".into(), content: "2".into() },
                    ParsedOption { label: "C".into(), content: "3".into() },
                    ParsedOption { label: "D".into(), content: "4".into() },
                ])
            } else {
                Some(vec![])
            },
            correct_answer: None,
            analysis: vec![],
            knowledge_points: vec![],
            confidence: 0.9,
            warnings: vec![],
            image_placeholders: vec![],
            image_urls: vec![],
            kp_matches: vec![],
            question_no: None,
            display_order: None,
            score: None,
            chapter_path: vec![],
            solution_methods: vec![],
        }
    }

    #[test]
    fn strips_multiline_options_residue() {
        let mut q = make_choice("下列结论正确的是\nA. 1\nB. 2\nC. 3\nD. 4", true);
        strip_options_residue_from_stem(&mut q);
        assert_eq!(q.stem, "下列结论正确的是");
        assert!(q.warnings.iter().any(|w| w.contains("剥离")));
    }

    #[test]
    fn normalizes_string_solution_methods() {
        // LLM 退化输出：字符串数组 → 包装为 {name} 对象数组
        let v = serde_json::json!({ "solution_methods": ["数形结合", "分类讨论"] });
        let out = normalize_solution_methods(v);
        assert_eq!(
            out["solution_methods"],
            serde_json::json!([{ "name": "数形结合" }, { "name": "分类讨论" }])
        );
        // 已是对象数组 → 原样返回
        let v2 = serde_json::json!({ "solution_methods": [{ "name": "换元法", "confidence": 0.9 }] });
        let out2 = normalize_solution_methods(v2);
        assert_eq!(out2["solution_methods"][0]["name"], "换元法");
        // 字段缺失 → 不动
        let v3 = serde_json::json!({ "stem": "x" });
        let out3 = normalize_solution_methods(v3);
        assert!(out3.get("solution_methods").is_none());
        // 归一化后可整体反序列化为 ParsedQuestion
        let full = serde_json::json!({
            "question_type": "solution", "stem": "求值", "analysis": [],
            "knowledge_points": [], "confidence": 0.9, "warnings": [],
            "image_placeholders": [], "solution_methods": ["配方法"]
        });
        let q: ParsedQuestion =
            serde_json::from_value(normalize_solution_methods(full)).unwrap();
        assert_eq!(q.solution_methods[0].name, "配方法");
    }

    #[test]
    fn strips_inline_options_after_period() {
        let mut q = make_choice("求 x 的值。 A. 1 B. 2 C. 3 D. 4", true);
        strip_options_residue_from_stem(&mut q);
        assert_eq!(q.stem, "求 x 的值");
    }

    #[test]
    fn strips_with_chinese_comma_separators() {
        // 选项前缀用中文顿号「A、」
        let mut q = make_choice(
            "下列哪个正确\nA、选项一\nB、选项二\nC、选项三\nD、选项四",
            true,
        );
        strip_options_residue_from_stem(&mut q);
        assert_eq!(q.stem, "下列哪个正确");
    }

    #[test]
    fn keeps_stem_without_residue() {
        let mut q = make_choice("下列结论正确的是", true);
        strip_options_residue_from_stem(&mut q);
        assert_eq!(q.stem, "下列结论正确的是");
        assert!(q.warnings.is_empty());
    }

    #[test]
    fn skips_non_choice_question() {
        let mut q = make_choice("求 x\nA. 1\nB. 2\nC. 3\nD. 4", true);
        q.question_type = "solution".into();
        strip_options_residue_from_stem(&mut q);
        // solution 题型不应剥离
        assert!(q.stem.contains("A. 1"));
        assert!(q.warnings.is_empty());
    }

    #[test]
    fn skips_when_options_empty_to_avoid_data_loss() {
        // options 为空时剥离会丢失唯一选项副本 → 不剥
        let mut q = make_choice("求 x\nA. 1\nB. 2\nC. 3\nD. 4", false);
        strip_options_residue_from_stem(&mut q);
        assert!(q.stem.contains("A. 1"));
        assert!(q.warnings.is_empty());
    }

    #[test]
    fn preserves_inline_a_in_stem_while_stripping_trailing_block() {
        // 题干正文含 "点 A."，尾部有真正选项块——只剥尾部，保留 "点 A."
        // （"点 A." 前是空格，非行首/句末标点，不满足分隔符条件）
        let mut q = make_choice("已知点 A. 在第一象限\nA. 1\nB. 2\nC. 3\nD. 4", true);
        strip_options_residue_from_stem(&mut q);
        assert_eq!(q.stem, "已知点 A. 在第一象限");
    }

    #[test]
    fn does_not_strip_partial_options_without_d() {
        // 只有 A B C，没有 D → 不匹配完整序列，不剥离
        let mut q = make_choice("求 x\nA. 1\nB. 2\nC. 3", true);
        strip_options_residue_from_stem(&mut q);
        assert!(q.stem.contains("A. 1"));
        assert!(q.warnings.is_empty());
    }

    #[test]
    fn replaces_trailing_empty_parens_with_hspace() {
        let mut q = make_choice("已知全集 $U=R$，如图阴影部分表示的集合是 ()", true);
        normalize_choice_answer_blank(&mut q);
        assert_eq!(
            q.stem,
            "已知全集 $U=R$，如图阴影部分表示的集合是 $(\\hspace{2em})$"
        );
    }

    #[test]
    fn replaces_fullwidth_empty_parens_before_stem_image() {
        let mut q = make_choice("下列图象可能是（　）\n![配图](/uploads/questions/a.jpg)", true);
        normalize_choice_answer_blank(&mut q);
        assert!(q.stem.starts_with("下列图象可能是$(\\hspace{2em})$"));
        assert!(q.stem.contains("![配图](/uploads/questions/a.jpg)"));
    }

    #[test]
    fn does_not_replace_function_call_parens() {
        let mut q = make_choice("已知 $f(x)=x$，则 $f()$ 的值是", true);
        normalize_choice_answer_blank(&mut q);
        assert_eq!(q.stem, "已知 $f(x)=x$，则 $f()$ 的值是");
    }

    #[test]
    fn does_not_replace_subquestion_numbers() {
        let mut q = make_choice("阅读材料。\n(1) 求值；(2) 求范围。", true);
        normalize_choice_answer_blank(&mut q);
        assert!(q.stem.contains("(1)"));
        assert!(q.stem.contains("(2)"));
        assert!(!q.stem.contains("\\hspace{2em}"));
    }

    #[test]
    fn skips_fill_question_empty_parens() {
        let mut q = make_choice("填空：()", true);
        q.question_type = "fill".into();
        normalize_choice_answer_blank(&mut q);
        assert_eq!(q.stem, "填空：()");
    }

    // -----------------------------------------------------------------------
    // OCR 引擎决策来源判定：ocr_decision_source（决策日志 decision_source 字段）
    // -----------------------------------------------------------------------

    #[test]
    fn test_decision_source_task_override_takes_priority() {
        // 任务显式指定引擎 → 恒为 task_override，即使与用户偏好相同
        assert_eq!(ocr_decision_source(Some("doc2x"), Some("mineru_local")), "task_override");
        assert_eq!(ocr_decision_source(Some("doc2x"), Some("doc2x")), "task_override");
        // 显式 auto 也算任务级覆盖（语义：本任务强制 auto）
        assert_eq!(ocr_decision_source(Some("auto"), Some("doc2x")), "task_override");
        // 无偏好时 override 依然最高
        assert_eq!(ocr_decision_source(Some("qwen_vl"), None), "task_override");
    }

    #[test]
    fn test_decision_source_user_preference_when_no_override() {
        // 无 override：非 auto 偏好 → user_preference
        assert_eq!(ocr_decision_source(None, Some("doc2x")), "user_preference");
        assert_eq!(ocr_decision_source(None, Some("mineru_local")), "user_preference");
        // 显式偏好 qwen_vl 同样是用户决策（区别于 auto 兜底）
        assert_eq!(ocr_decision_source(None, Some("qwen_vl")), "user_preference");
    }

    #[test]
    fn test_decision_source_auto_default_fallback() {
        // 偏好为 auto / 无设置行 → auto_default
        assert_eq!(ocr_decision_source(None, Some("auto")), "auto_default");
        assert_eq!(ocr_decision_source(None, None), "auto_default");
    }
}
