//! 导出端点（模块 A 的 HTTP 入口）— T1.7
//!
//! `POST /export/markdown[?bundle=true]`：
//! - 装配（`assemble_exam`：批量取题 + 可见性过滤）→ 生成 Markdown / zip；
//! - `Content-Disposition` RFC 5987 编码中文文件名（§四）；
//! - `X-Export-Warnings` 头返回降级警告（URL-encoded JSON `[{question_no,
//!   field, latex, reason}]`）；序列化超 ~8KB 截断并附 `truncated:true`
//!   标记（B3），前端提示改用预览接口查看完整清单。

use std::path::Path;

use axum::{
    extract::{Query, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::Deserialize;
use serde_json::json;

use crate::auth::middleware::AuthUser;
use crate::export::assembler::assemble_exam;
use crate::export::markdown::generate_markdown;
use crate::export::model::{ExamBundle, ExamRequest, Issue};
use crate::handlers::questions::db_err;
use crate::AppState;

/// `X-Export-Warnings` 编码后长度上限（B3：代理默认 ~8KB 头限制，留余量）
const WARNINGS_HEADER_LIMIT: usize = 8000;

#[derive(Debug, Default, Deserialize)]
pub struct MarkdownQuery {
    /// `?bundle=true` → 打包 zip（md + images/）
    #[serde(default)]
    pub bundle: Option<bool>,
}

/// POST /api/v1/export/markdown — Markdown 导出（T1.7）
pub async fn export_markdown(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthUser>,
    Query(q): Query<MarkdownQuery>,
    Json(req): Json<ExamRequest>,
) -> Response {
    let make_zip = q.bundle.unwrap_or(false);

    // 1. 装配：批量取题 + 可见性过滤（不可见/不存在题跳过并记警告）
    let assembled = match assemble_exam(&state.pool, &auth, &req).await {
        Ok(a) => a,
        Err(e) => return db_err(e.to_string()).into_response(),
    };

    // 2. 生成（bundle=true 时抓取图片并打 zip；抓取失败降级记警告）
    let result = generate_markdown(
        &assembled.bundle,
        &req.options,
        Path::new(&state.upload_dir),
        make_zip,
    )
    .await;

    // 3. 合并警告：卷级 + 题级 + 生成期（图片抓取）
    let mut issues = assembled.issues;
    collect_question_issues(&assembled.bundle, &mut issues);
    issues.extend(result.issues);

    let (content_type, ext, body) = if make_zip {
        let zip = result.zip.unwrap_or_default();
        (
            HeaderValue::from_static("application/zip"),
            "zip",
            zip,
        )
    } else {
        (
            HeaderValue::from_static("text/markdown; charset=utf-8"),
            "md",
            result.markdown.into_bytes(),
        )
    };

    let filename = sanitize_filename(&assembled.bundle.title);

    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, content_type);
    if let Ok(v) = HeaderValue::from_str(&content_disposition(&filename, ext)) {
        headers.insert(header::CONTENT_DISPOSITION, v);
    }
    if let Some(v) = warnings_header_value(&issues) {
        headers.insert("X-Export-Warnings", v);
    }

    (StatusCode::OK, headers, body).into_response()
}

/// 收集各题携带的题级 issues 到卷级清单
fn collect_question_issues(bundle: &ExamBundle, out: &mut Vec<Issue>) {
    for s in &bundle.sections {
        for q in &s.questions {
            out.extend(q.issues.iter().cloned());
        }
    }
}

// ═══════════════════════════ 文件名（RFC 5987） ═══════════════════════════

/// 剔除文件系统/HTTP 头不安全字符，超长截断，空标题兜底
fn sanitize_filename(title: &str) -> String {
    let cleaned: String = title
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect::<String>()
        .trim()
        .to_string();
    let mut s = if cleaned.is_empty() {
        "试卷".to_string()
    } else {
        cleaned
    };
    // 按字符截断到 60（UTF-8 后 ~180 字节，任何编码下文件名都安全）
    if s.chars().count() > 60 {
        s = s.chars().take(60).collect();
    }
    s
}

/// `attachment; filename="ascii-fallback"; filename*=UTF-8''<pct-encoded>`
fn content_disposition(name: &str, ext: &str) -> String {
    // ASCII 回退名：仅保留安全可见字符，否则用通用名
    let fallback: String = name
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ' '))
        .collect();
    let fallback = if fallback.trim().is_empty() {
        "exam".to_string()
    } else {
        fallback.trim().to_string()
    };
    format!(
        "attachment; filename=\"{}.{}\"; filename*=UTF-8''{}.{}",
        fallback,
        ext,
        percent_encode(name.as_bytes()),
        ext
    )
}

// ═══════════════════════════ 警告头（B3 截断） ═══════════════════════════

/// 警告头值：URL-encoded JSON 数组 `[{question_no, field, latex, reason}]`；
/// **按编码后长度**判定超限（中文 percent-encoding 膨胀 3 倍，按原始长度算会放大上限），
/// 超限时按整条丢弃截断并追加 `truncated:true` 哨兵项。
fn warnings_header_value(issues: &[Issue]) -> Option<HeaderValue> {
    if issues.is_empty() {
        return None;
    }
    let items: Vec<serde_json::Value> = issues.iter().map(warning_item_json).collect();

    if let Some(s) = encoded_json(&items) {
        if s.len() <= WARNINGS_HEADER_LIMIT {
            return HeaderValue::from_str(&s).ok();
        }
    }

    // 超限：逐条追加，任何时刻都保证「含哨兵项」仍在上限内
    let mut kept: Vec<serde_json::Value> = Vec::new();
    for item in items {
        kept.push(item);
        kept.push(sentinel_warning());
        let fits = encoded_json(&kept)
            .map(|s| s.len() <= WARNINGS_HEADER_LIMIT)
            .unwrap_or(false);
        if !fits {
            kept.truncate(kept.len() - 2);
            break;
        }
        kept.pop(); // 去掉哨兵，下一轮重新试探
    }
    // 一条真实警告都放不下时，kept 为空，仅回哨兵项
    kept.push(sentinel_warning());
    encoded_json(&kept).and_then(|s| HeaderValue::from_str(&s).ok())
}

/// 序列化并 percent-encode 为可直接入头的字符串（ASCII）
fn encoded_json(items: &[serde_json::Value]) -> Option<String> {
    serde_json::to_string(items)
        .ok()
        .map(|s| percent_encode(s.as_bytes()))
}

fn sentinel_warning() -> serde_json::Value {
    json!({
        "field": "other",
        "reason": "警告清单超过长度上限已截断，完整清单请用预览接口查看",
        "truncated": true
    })
}

/// 单条警告 → 计划 §四 契约的四字段 JSON（severity 不进文件响应头）
fn warning_item_json(i: &Issue) -> serde_json::Value {
    json!({
        "question_no": i.question_no,
        "field": i.field,
        "latex": i.latex,
        "reason": i.reason,
    })
}

// ═══════════════════════════ 百分号编码 ═══════════════════════════

/// RFC 3986 percent-encoding：非 unreserved（`A-Za-z0-9-_.~`）字节编码为 %XX。
/// 输出恒为 ASCII，可安全置于 HTTP 头。
fn percent_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    for b in bytes {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

// ═══════════════════════════ 单元测试 ═══════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::model::{IssueField, IssueSeverity};

    fn issue(reason: &str) -> Issue {
        Issue {
            question_no: Some(3),
            field: IssueField::Stem,
            severity: IssueSeverity::Warning,
            latex: None,
            reason: reason.to_string(),
        }
    }

    #[test]
    fn disposition_rfc5987_chinese() {
        let d = content_disposition("集合单元测验", "md");
        assert!(d.starts_with("attachment; filename=\"exam.md\""), "{}", d);
        // 汉字 → UTF-8 百分号编码（"集" = E9 9B 86）
        assert!(d.contains("filename*=UTF-8''%E9%9B%86"), "{}", d);
        // ASCII 安全字符保留为回退名
        let d2 = content_disposition("Midterm Exam", "zip");
        assert!(d2.contains("filename=\"Midterm Exam.zip\""), "{}", d2);
        assert!(d2.contains("filename*=UTF-8''Midterm%20Exam.zip"), "{}", d2);
    }

    #[test]
    fn sanitize_removes_unsafe_chars() {
        assert_eq!(sanitize_filename("a/b\\c:d*e?f\"g<h>i|j"), "abcdefghij");
        assert_eq!(sanitize_filename("   "), "试卷");
        let long = "很".repeat(100);
        assert_eq!(sanitize_filename(&long).chars().count(), 60);
    }

    #[test]
    fn warnings_header_percent_encoded_json() {
        let v = warnings_header_value(&[issue("图片处理失败")]).unwrap();
        let s = v.to_str().unwrap();
        // 非保留字符全部编码，输出 ASCII
        assert!(s.starts_with("%5B%7B"), "{}", s);
        assert!(s.contains("%E5%9B%BE%E7%89%87"), "{}", s); // "图片"
    }

    #[test]
    fn warnings_header_empty_is_none() {
        assert!(warnings_header_value(&[]).is_none());
    }

    #[test]
    fn warnings_header_truncates_over_limit() {
        // 构造远超 8KB 的警告列表
        let issues: Vec<Issue> = (0..200)
            .map(|i| issue(&format!("警告编号 {}：{}", i, "很长的原因".repeat(20))))
            .collect();
        let v = warnings_header_value(&issues).unwrap();
        let encoded = v.to_str().unwrap();
        // 契约：头值本身（percent-encoded 后）必须落在上限内，而非原始 JSON 长度
        assert!(
            encoded.len() <= WARNINGS_HEADER_LIMIT,
            "encoded len {} exceeds limit",
            encoded.len()
        );
        // 解码回 JSON 验证哨兵项
        let decoded = percent_decode(encoded);
        let arr: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        let last = arr.as_array().unwrap().last().unwrap();
        assert_eq!(last["truncated"], json!(true));
        assert!(last["reason"].as_str().unwrap().contains("截断"));
        // 截断后必须丢掉了部分条目
        assert!(arr.as_array().unwrap().len() < 200);
    }

    #[test]
    fn warnings_header_under_limit_keeps_all() {
        let issues: Vec<Issue> = (0..5).map(|i| issue(&format!("警告 {}", i))).collect();
        let v = warnings_header_value(&issues).unwrap();
        let decoded = percent_decode(v.to_str().unwrap());
        let arr: serde_json::Value = serde_json::from_str(&decoded).unwrap();
        assert_eq!(arr.as_array().unwrap().len(), 5);
        assert!(arr.as_array().unwrap().iter().all(|x| x.get("truncated").is_none()));
    }

    #[test]
    fn warnings_header_single_oversized_keeps_only_sentinel() {
        // 单条即超上限 → 退化为仅哨兵项，仍必须是合法且短小的头值
        let issues = vec![issue(&"很长的原因".repeat(2000))];
        let v = warnings_header_value(&issues).unwrap();
        let encoded = v.to_str().unwrap();
        assert!(encoded.len() <= WARNINGS_HEADER_LIMIT);
        let arr: serde_json::Value = serde_json::from_str(&percent_decode(encoded)).unwrap();
        let arr = arr.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["truncated"], json!(true));
    }

    /// 测试辅助：逆向 percent_encode
    fn percent_decode(s: &str) -> String {
        let bytes = s.as_bytes();
        let mut out = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'%' && i + 2 < bytes.len() {
                let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                i += 3;
            } else {
                out.push(bytes[i]);
                i += 1;
            }
        }
        String::from_utf8(out).unwrap()
    }
}
