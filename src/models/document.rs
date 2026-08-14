// ===========================================================================
// V2.1.1 资料类型扩展：Document 数据模型
//
// 方案 A（TD-3）：documents.id 即文件实体 ID，不设 file_id 列。
// document_type 为 TEXT + 白名单校验（TD-2），详见计划书 §四/§五。
// ===========================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 类型白名单（TD-2：TEXT + 后端校验，不用 PG enum）
// ---------------------------------------------------------------------------

/// DocumentType：文件整体是什么（16 类）
pub const DOCUMENT_TYPES: [&str; 16] = [
    "exam",
    "mock_exam",
    "class_exercise",
    "class_example",
    "homework",
    "preview_exercise",
    "textbook_example",
    "teaching_material",
    "exercise_book",
    "chapter_exercise",
    "unit_exercise",
    "special_training",
    "wrong_question",
    "mixed",
    "unknown",
    "other",
];

/// CollectionType：这一组题是什么（不含 exam/mock_exam/mixed/unknown/other 除外）
pub const COLLECTION_TYPES: [&str; 12] = [
    "class_exercise",
    "class_example",
    "homework",
    "preview_exercise",
    "textbook_example",
    "teaching_material",
    "exercise_book",
    "chapter_exercise",
    "unit_exercise",
    "special_training",
    "wrong_question",
    "other",
];

pub fn is_valid_document_type(t: &str) -> bool {
    DOCUMENT_TYPES.contains(&t)
}

pub fn is_valid_collection_type(t: &str) -> bool {
    COLLECTION_TYPES.contains(&t)
}

/// 是否"试卷类"资料类型（confirm 后创建 Paper 而非 Collection）
pub fn is_paper_type(t: &str) -> bool {
    matches!(t, "exam" | "mock_exam")
}

/// 非试卷 document_type → 默认 collection_type 映射（同名；other → other）
pub fn default_collection_type_for(document_type: &str) -> Option<&'static str> {
    COLLECTION_TYPES.iter().find(|t| **t == document_type).copied()
}

// ---------------------------------------------------------------------------
// 实体
// ---------------------------------------------------------------------------

/// 资料/Document（数据库行）
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Document {
    pub id: Uuid,
    pub creator_id: Uuid,
    /// 原始文件名（仅展示用，不用于磁盘路径）
    pub file_name: String,
    pub file_size: Option<i64>,
    pub mime: Option<String>,
    pub page_count: i32,
    /// 业务资料类型（AI 推荐 → 用户确认后落库；confirmed 前为 NULL）
    pub document_type: Option<String>,
    /// document_type = 'other' 时的自定义类型名
    pub type_label: Option<String>,
    pub title: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    /// uploaded/classifying/classified/confirmed/parsing/done/failed/cancelled
    pub status: String,
    /// AI 分类结果：{document_type,title,confidence,reason,level,checked_pages}
    pub ai_classification: Option<serde_json::Value>,
    /// 扩展信息：confirm 后保存 paper_meta 快照与 collections 快照；
    /// 上传后保存 pages 文件清单 ["page_1.webp", ...]
    pub metadata: serde_json::Value,
    /// TD-1：PDF 转换引擎标识（pdfjs / doc2x / mineru）
    pub conversion_engine: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// 请求 / 响应类型
// ---------------------------------------------------------------------------

/// Paper 元数据（用户确认时输入，Worker 落库 papers 用；同时作为快照）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PaperMetaInput {
    pub title: String,
    pub year: Option<i32>,
    pub stage: Option<String>,
    pub grade: Option<String>,
    pub subject: Option<String>,
    pub semester: Option<String>,
    pub region_province: Option<String>,
    pub region_city: Option<String>,
    pub school_name: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    /// 用户显式选择"关联已有试卷"（计划书 §6.1 复用规则 (b)）
    pub paper_id: Option<Uuid>,
}

/// Collection 元数据（用户确认时输入，Worker 落库 question_collections 用）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CollectionMetaInput {
    pub title: String,
    pub collection_type: String,
    /// collection_type = 'other' 时的自定义名
    pub type_label: Option<String>,
    pub source_type: Option<String>,
    pub subject: Option<String>,
    pub stage: Option<String>,
    pub grade: Option<String>,
    pub semester: Option<String>,
    /// 章节（知识树节点，可选）
    pub chapter_id: Option<Uuid>,
}

/// POST /ai/documents/{id}/confirm 请求体
#[derive(Debug, Deserialize)]
pub struct ConfirmDocumentRequest {
    /// 最终资料类型（白名单内；unknown 不允许提交，前端必须让用户选择）
    pub document_type: String,
    /// document_type = 'other' 时必填
    pub type_label: Option<String>,
    pub title: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    /// exam / mock_exam 必填
    pub paper_meta: Option<PaperMetaInput>,
    /// mixed 必填（≥1 个集合壳）；其他非试卷类型可省略（自动建默认单集合）
    pub collections: Option<Vec<CollectionMetaInput>>,
}

/// AI 分类原始输出（LLM 返回，未含 level/checked_pages）
#[derive(Debug, Deserialize, Clone)]
pub struct AiClassificationRaw {
    pub document_type: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub confidence: f32,
    #[serde(default)]
    pub reason: Option<String>,
}

/// AI 分类最终结果（入库 ai_classification JSONB）
#[derive(Debug, Clone, Serialize)]
pub struct AiClassification {
    pub document_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// 命中的检测层级 1/2/3
    pub level: i32,
    /// 实际检查的页数
    pub checked_pages: i32,
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

/// confirm 请求校验；返回 (paper_meta, collections) 归一化后的结构
pub struct ConfirmNormalized {
    /// 归一化后的 collections 列表（非试卷类型为空时自动补默认单集合）
    pub collections: Vec<CollectionMetaInput>,
    /// 是否试卷类型（exam / mock_exam）
    pub is_paper: bool,
}

/// 校验 confirm 请求，返回归一化结果；Err 为给前端的错误信息
pub fn validate_confirm(req: &ConfirmDocumentRequest) -> Result<ConfirmNormalized, String> {
    let doc_type = req.document_type.trim();
    if !is_valid_document_type(doc_type) {
        return Err(format!("未知资料类型: {doc_type}"));
    }
    if doc_type == "unknown" {
        return Err("无法自动判断资料类型，请先选择资料类型".to_string());
    }
    if doc_type == "other" {
        let label = req.type_label.as_deref().unwrap_or("").trim();
        if label.is_empty() {
            return Err("选择「其他」类型时必须填写自定义类型名（type_label）".to_string());
        }
    }

    let is_paper = is_paper_type(doc_type);

    // 试卷类型：必须提供 paper_meta.title
    if is_paper {
        let meta = req
            .paper_meta
            .as_ref()
            .ok_or_else(|| "正式试卷 / 模拟试卷必须填写试卷信息（paper_meta）".to_string())?;
        if meta.title.trim().is_empty() {
            return Err("试卷名称（paper_meta.title）不能为空".to_string());
        }
        return Ok(ConfirmNormalized {
            collections: vec![],
            is_paper: true,
        });
    }

    // mixed：必须提供 ≥1 个集合
    if doc_type == "mixed" {
        let collections = req
            .collections
            .as_ref()
            .ok_or_else(|| "混合资料必须至少提供一个题目集合（collections）".to_string())?;
        if collections.is_empty() {
            return Err("混合资料必须至少提供一个题目集合（collections）".to_string());
        }
        validate_collections(collections)?;
        return Ok(ConfirmNormalized {
            collections: collections.clone(),
            is_paper: false,
        });
    }

    // 其他非试卷类型：collections 可省略 → 自动补默认单集合
    let mut collections = req.collections.clone().unwrap_or_default();
    if !collections.is_empty() {
        validate_collections(&collections)?;
    } else {
        let default_type = default_collection_type_for(doc_type)
            .ok_or_else(|| format!("资料类型 {doc_type} 无法映射到默认集合类型"))?;
        collections.push(CollectionMetaInput {
            title: req
                .title
                .clone()
                .filter(|t| !t.trim().is_empty())
                .unwrap_or_else(|| "默认题目集合".to_string()),
            collection_type: default_type.to_string(),
            type_label: if default_type == "other" {
                req.type_label.clone()
            } else {
                None
            },
            source_type: req.source_type.clone(),
            subject: None,
            stage: None,
            grade: None,
            semester: None,
            chapter_id: None,
        });
    }

    Ok(ConfirmNormalized {
        collections,
        is_paper: false,
    })
}

fn validate_collections(collections: &[CollectionMetaInput]) -> Result<(), String> {
    for c in collections {
        if c.title.trim().is_empty() {
            return Err("集合名称（title）不能为空".to_string());
        }
        if !is_valid_collection_type(&c.collection_type) {
            return Err(format!("未知集合类型: {}", c.collection_type));
        }
        if c.collection_type == "other" {
            let label = c.type_label.as_deref().unwrap_or("").trim();
            if label.is_empty() {
                return Err("集合类型为「其他」时必须填写自定义类型名（type_label）".to_string());
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 单元测试
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_document_type_whitelist() {
        assert!(is_valid_document_type("exam"));
        assert!(is_valid_document_type("mixed"));
        assert!(is_valid_document_type("unknown"));
        assert!(is_valid_document_type("other"));
        assert!(!is_valid_document_type("quiz"));
        assert!(!is_valid_document_type(""));
    }

    #[test]
    fn test_collection_type_whitelist() {
        assert!(is_valid_collection_type("class_exercise"));
        assert!(is_valid_collection_type("other"));
        // 试卷/混合/未知不是集合类型
        assert!(!is_valid_collection_type("exam"));
        assert!(!is_valid_collection_type("mixed"));
        assert!(!is_valid_collection_type("unknown"));
    }

    #[test]
    fn test_paper_type_detection() {
        assert!(is_paper_type("exam"));
        assert!(is_paper_type("mock_exam"));
        assert!(!is_paper_type("homework"));
    }

    #[test]
    fn test_default_collection_mapping() {
        assert_eq!(default_collection_type_for("homework"), Some("homework"));
        assert_eq!(default_collection_type_for("other"), Some("other"));
        assert_eq!(default_collection_type_for("exam"), None);
        assert_eq!(default_collection_type_for("mixed"), None);
    }

    #[test]
    fn test_validate_confirm_exam_requires_title() {
        let req = ConfirmDocumentRequest {
            document_type: "exam".into(),
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: None,
            paper_meta: None,
            collections: None,
        };
        assert!(validate_confirm(&req).is_err());

        let req2 = ConfirmDocumentRequest {
            document_type: "exam".into(),
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: None,
            paper_meta: Some(PaperMetaInput {
                title: "2025高一数学期中考试".into(),
                year: None,
                stage: None,
                grade: None,
                subject: None,
                semester: None,
                region_province: None,
                region_city: None,
                school_name: None,
                source_type: None,
                sub_source_type: None,
                paper_id: None,
            }),
            collections: None,
        };
        let n = validate_confirm(&req2).unwrap();
        assert!(n.is_paper);
    }

    #[test]
    fn test_validate_confirm_mixed_requires_collections() {
        let req = ConfirmDocumentRequest {
            document_type: "mixed".into(),
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: None,
            paper_meta: None,
            collections: None,
        };
        assert!(validate_confirm(&req).is_err());

        let req2 = ConfirmDocumentRequest {
            document_type: "mixed".into(),
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: None,
            paper_meta: None,
            collections: Some(vec![CollectionMetaInput {
                title: "课堂练习".into(),
                collection_type: "class_exercise".into(),
                type_label: None,
                source_type: None,
                subject: None,
                stage: None,
                grade: None,
                semester: None,
                chapter_id: None,
            }]),
        };
        assert!(validate_confirm(&req2).is_ok());
    }

    #[test]
    fn test_validate_confirm_other_requires_label() {
        let req = ConfirmDocumentRequest {
            document_type: "other".into(),
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: None,
            paper_meta: None,
            collections: None,
        };
        assert!(validate_confirm(&req).is_err());

        let req2 = ConfirmDocumentRequest {
            document_type: "other".into(),
            type_label: Some("校本资料".into()),
            title: Some("导数专题".into()),
            source_type: None,
            sub_source_type: None,
            paper_meta: None,
            collections: None,
        };
        let n = validate_confirm(&req2).unwrap();
        assert!(!n.is_paper);
        assert_eq!(n.collections.len(), 1);
        assert_eq!(n.collections[0].collection_type, "other");
        assert_eq!(n.collections[0].type_label.as_deref(), Some("校本资料"));
    }

    #[test]
    fn test_validate_confirm_unknown_rejected() {
        let req = ConfirmDocumentRequest {
            document_type: "unknown".into(),
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: None,
            paper_meta: None,
            collections: None,
        };
        assert!(validate_confirm(&req).is_err());
    }

    #[test]
    fn test_validate_confirm_default_collection_auto_fill() {
        let req = ConfirmDocumentRequest {
            document_type: "class_exercise".into(),
            type_label: None,
            title: Some("二次函数课堂练习".into()),
            source_type: Some("teacher_created".into()),
            sub_source_type: None,
            paper_meta: None,
            collections: None,
        };
        let n = validate_confirm(&req).unwrap();
        assert_eq!(n.collections.len(), 1);
        assert_eq!(n.collections[0].title, "二次函数课堂练习");
        assert_eq!(n.collections[0].collection_type, "class_exercise");
        assert_eq!(n.collections[0].source_type.as_deref(), Some("teacher_created"));
    }
}
