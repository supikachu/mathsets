// ===========================================================================
// V2.1.1 资料类型扩展：Document 数据模型
//
// 方案 A（TD-3）：documents.id 即文件实体 ID，不设 file_id 列。
// 来源改为「大类 source_category + 子类 source_kind」级联；
// document_type 列保留作兼容（写入 category:kind 或旧值映射）。
// ===========================================================================

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// 来源级联白名单（TEXT + 后端校验）
// ---------------------------------------------------------------------------

/// 来源大类
pub const SOURCE_CATEGORIES: [&str; 3] = ["paper", "practice", "other"];

/// 试卷子类
pub const PAPER_KINDS: [&str; 7] = [
    "monthly_test",
    "unit_test",
    "stage_test",
    "midterm",
    "final",
    "gaokao",
    "mock",
];

/// 练习子类
pub const PRACTICE_KINDS: [&str; 5] = [
    "preview",
    "class_example",
    "in_class",
    "homework",
    "unit_review",
];

/// 其他子类
pub const OTHER_KINDS: [&str; 5] = [
    "special",
    "workbook",
    "textbook_example",
    "lecture",
    "wrong_question",
];

/// 旧扁平 DocumentType（兼容读 / 映射）
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

/// CollectionType（保留；方案 A 下练习/其他不再自动建集合）
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

pub fn is_valid_source_category(c: &str) -> bool {
    SOURCE_CATEGORIES.contains(&c)
}

pub fn is_valid_source_kind(kind: &str) -> bool {
    PAPER_KINDS.contains(&kind) || PRACTICE_KINDS.contains(&kind) || OTHER_KINDS.contains(&kind)
}

pub fn source_kind_matches_category(category: &str, kind: &str) -> bool {
    match category {
        "paper" => PAPER_KINDS.contains(&kind),
        "practice" => PRACTICE_KINDS.contains(&kind),
        "other" => OTHER_KINDS.contains(&kind),
        _ => false,
    }
}

pub fn is_valid_document_type(t: &str) -> bool {
    DOCUMENT_TYPES.contains(&t)
        || is_valid_source_kind(t)
        || t.contains(':')
            && t.split_once(':')
                .is_some_and(|(c, k)| is_valid_source_category(c) && source_kind_matches_category(c, k))
}

pub fn is_valid_collection_type(t: &str) -> bool {
    COLLECTION_TYPES.contains(&t)
}

/// 旧 document_type → (category, kind)
pub fn map_legacy_document_type(t: &str) -> (String, String) {
    match t {
        "exam" => ("paper".into(), "monthly_test".into()),
        "mock_exam" => ("paper".into(), "mock".into()),
        "preview_exercise" => ("practice".into(), "preview".into()),
        "class_example" => ("practice".into(), "class_example".into()),
        "class_exercise" => ("practice".into(), "in_class".into()),
        "homework" => ("practice".into(), "homework".into()),
        "unit_exercise" => ("practice".into(), "unit_review".into()),
        "chapter_exercise" | "special_training" => ("other".into(), "special".into()),
        "exercise_book" => ("other".into(), "workbook".into()),
        "textbook_example" => ("other".into(), "textbook_example".into()),
        "teaching_material" => ("other".into(), "lecture".into()),
        "wrong_question" => ("other".into(), "wrong_question".into()),
        "mixed" | "unknown" | "other" | "" => ("practice".into(), "in_class".into()),
        // 已是新 slug
        k if PAPER_KINDS.contains(&k) => ("paper".into(), k.into()),
        k if PRACTICE_KINDS.contains(&k) => ("practice".into(), k.into()),
        k if OTHER_KINDS.contains(&k) => ("other".into(), k.into()),
        // category:kind
        s if s.contains(':') => {
            let (c, k) = s.split_once(':').unwrap();
            if is_valid_source_category(c) && source_kind_matches_category(c, k) {
                (c.into(), k.into())
            } else {
                ("practice".into(), "in_class".into())
            }
        }
        _ => ("practice".into(), "in_class".into()),
    }
}

/// 兼容列写入值：`paper:mock` 形式
pub fn document_type_compat(category: &str, kind: &str) -> String {
    format!("{category}:{kind}")
}

/// 是否应创建试卷实体：大类为试卷且 create_paper=true
pub fn should_create_paper(category: &str, create_paper: bool) -> bool {
    category == "paper" && create_paper
}

/// 旧 is_paper_type：兼容 exam/mock_exam 与 paper:*
pub fn is_paper_type(t: &str) -> bool {
    matches!(t, "exam" | "mock_exam")
        || t.starts_with("paper:")
        || PAPER_KINDS.contains(&t)
}

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
    /// 兼容列：新写入为 `category:kind`；旧行为扁平 16 类
    pub document_type: Option<String>,
    /// document_type = 'other' 时的自定义类型名（旧）
    pub type_label: Option<String>,
    pub title: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    /// uploaded/classifying/classified/confirmed/parsing/done/failed/cancelled
    pub status: String,
    /// AI 分类结果
    pub ai_classification: Option<serde_json::Value>,
    /// 扩展信息：paper_meta / collections / source_category / source_kind / create_paper / pages
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
    /// 用户显式选择"关联已有试卷"
    pub paper_id: Option<Uuid>,
}

impl PaperMetaInput {
    /// 试卷类型：表单 source_type，否则回退到来源子类（如 final / midterm）
    pub fn resolved_source_type(&self, fallback_kind: &str) -> Option<String> {
        self.source_type
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .or_else(|| {
                let k = fallback_kind.trim();
                if k.is_empty() {
                    None
                } else {
                    Some(k.to_string())
                }
            })
    }
}

/// Collection 元数据（保留兼容；方案 A 下默认不建集合）
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CollectionMetaInput {
    pub title: String,
    pub collection_type: String,
    pub type_label: Option<String>,
    pub source_type: Option<String>,
    pub subject: Option<String>,
    pub stage: Option<String>,
    pub grade: Option<String>,
    pub semester: Option<String>,
    pub chapter_id: Option<Uuid>,
}

/// POST /ai/documents/{id}/confirm 请求体（级联来源）
#[derive(Debug, Deserialize)]
pub struct ConfirmDocumentRequest {
    /// 来源大类：paper | practice | other（优先）
    #[serde(default)]
    pub source_category: Option<String>,
    /// 来源子类 slug
    #[serde(default)]
    pub source_kind: Option<String>,
    /// 是否创建试卷实体（仅 paper 有效）
    #[serde(default)]
    pub create_paper: Option<bool>,
    /// 兼容旧前端：扁平 document_type
    #[serde(default)]
    pub document_type: Option<String>,
    pub type_label: Option<String>,
    pub title: Option<String>,
    pub source_type: Option<String>,
    pub sub_source_type: Option<String>,
    /// create_paper=true 时必填 title（或关联 paper_id）
    pub paper_meta: Option<PaperMetaInput>,
    /// 兼容旧 mixed；方案 A 下通常为空
    pub collections: Option<Vec<CollectionMetaInput>>,
}

/// AI 分类原始输出
#[derive(Debug, Deserialize, Clone)]
pub struct AiClassificationRaw {
    /// 新字段
    #[serde(default)]
    pub source_category: Option<String>,
    #[serde(default)]
    pub source_kind: Option<String>,
    /// 旧字段兼容
    #[serde(default)]
    pub document_type: Option<String>,
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
    pub source_category: String,
    pub source_kind: String,
    /// 兼容列同步值
    pub document_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub confidence: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub level: i32,
    pub checked_pages: i32,
}

impl AiClassificationRaw {
    /// 归一化为 (category, kind)；非法/低置信 → practice/in_class
    pub fn resolve_source(&self) -> (String, String) {
        if let (Some(c), Some(k)) = (&self.source_category, &self.source_kind) {
            let c = c.trim();
            let k = k.trim();
            if is_valid_source_category(c) && source_kind_matches_category(c, k) {
                return (c.into(), k.into());
            }
        }
        if let Some(dt) = &self.document_type {
            return map_legacy_document_type(dt.trim());
        }
        ("practice".into(), "in_class".into())
    }
}

// ---------------------------------------------------------------------------
// 校验
// ---------------------------------------------------------------------------

pub struct ConfirmNormalized {
    pub source_category: String,
    pub source_kind: String,
    pub create_paper: bool,
    /// 兼容写入 documents.document_type
    pub document_type: String,
    pub collections: Vec<CollectionMetaInput>,
    /// 是否应创建/关联试卷
    pub is_paper: bool,
}

/// 校验 confirm 请求
pub fn validate_confirm(req: &ConfirmDocumentRequest) -> Result<ConfirmNormalized, String> {
    let (category, kind) = resolve_confirm_source(req)?;

    let create_paper = req.create_paper.unwrap_or(false) && category == "paper";
    let document_type = document_type_compat(&category, &kind);

    // 创建试卷：需 paper_meta.title 或关联 paper_id
    if create_paper {
        let meta = req
            .paper_meta
            .as_ref()
            .ok_or_else(|| "创建试卷时请填写试卷信息（paper_meta）".to_string())?;
        let has_link = meta.paper_id.is_some();
        if !has_link && meta.title.trim().is_empty() {
            return Err("创建试卷时名称不能为空".to_string());
        }
        return Ok(ConfirmNormalized {
            source_category: category,
            source_kind: kind,
            create_paper: true,
            document_type,
            collections: vec![],
            is_paper: true,
        });
    }

    // 试卷但不建卷、练习、其他：独立题，不建集合
    let collections = req.collections.clone().unwrap_or_default();
    if !collections.is_empty() {
        validate_collections(&collections)?;
    }

    Ok(ConfirmNormalized {
        source_category: category,
        source_kind: kind,
        create_paper: false,
        document_type,
        collections,
        is_paper: false,
    })
}

fn resolve_confirm_source(req: &ConfirmDocumentRequest) -> Result<(String, String), String> {
    if let (Some(c), Some(k)) = (&req.source_category, &req.source_kind) {
        let c = c.trim();
        let k = k.trim();
        if !is_valid_source_category(c) {
            return Err(format!("未知来源大类: {c}"));
        }
        if !source_kind_matches_category(c, k) {
            return Err(format!("子类 {k} 不属于大类 {c}"));
        }
        return Ok((c.into(), k.into()));
    }
    // 兼容旧扁平 document_type
    if let Some(dt) = req.document_type.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if dt == "unknown" {
            return Err("无法自动判断资料类型，请先选择资料类型".to_string());
        }
        if dt == "mixed" {
            return Err("已不再支持混合资料类型，请选择试卷/练习/其他".to_string());
        }
        if dt == "other" {
            // 旧 other 需 type_label；映射到 other/special
            let label = req.type_label.as_deref().unwrap_or("").trim();
            if label.is_empty() {
                return Err("选择「其他」类型时必须填写自定义类型名（type_label）".to_string());
            }
            return Ok(("other".into(), "special".into()));
        }
        if is_valid_document_type(dt) || is_valid_source_kind(dt) {
            return Ok(map_legacy_document_type(dt));
        }
        return Err(format!("未知资料类型: {dt}"));
    }
    Err("请选择来源大类与子类".to_string())
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
    fn test_source_cascade_whitelist() {
        assert!(is_valid_source_category("paper"));
        assert!(source_kind_matches_category("paper", "mock"));
        assert!(source_kind_matches_category("practice", "in_class"));
        assert!(source_kind_matches_category("other", "lecture"));
        assert!(!source_kind_matches_category("paper", "homework"));
    }

    #[test]
    fn test_legacy_mapping() {
        assert_eq!(map_legacy_document_type("mock_exam"), ("paper".into(), "mock".into()));
        assert_eq!(map_legacy_document_type("class_exercise"), ("practice".into(), "in_class".into()));
        assert_eq!(map_legacy_document_type("wrong_question"), ("other".into(), "wrong_question".into()));
        assert_eq!(map_legacy_document_type("unknown"), ("practice".into(), "in_class".into()));
    }

    #[test]
    fn test_validate_confirm_cascade_practice_no_collection() {
        let req = ConfirmDocumentRequest {
            source_category: Some("practice".into()),
            source_kind: Some("in_class".into()),
            create_paper: Some(false),
            document_type: None,
            type_label: None,
            title: Some("练习".into()),
            source_type: None,
            sub_source_type: None,
            paper_meta: None,
            collections: None,
        };
        let n = validate_confirm(&req).unwrap();
        assert!(!n.is_paper);
        assert!(!n.create_paper);
        assert!(n.collections.is_empty());
        assert_eq!(n.document_type, "practice:in_class");
    }

    #[test]
    fn test_validate_confirm_create_paper_requires_title() {
        let req = ConfirmDocumentRequest {
            source_category: Some("paper".into()),
            source_kind: Some("midterm".into()),
            create_paper: Some(true),
            document_type: None,
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: None,
            paper_meta: None,
            collections: None,
        };
        assert!(validate_confirm(&req).is_err());

        let req2 = ConfirmDocumentRequest {
            source_category: Some("paper".into()),
            source_kind: Some("midterm".into()),
            create_paper: Some(true),
            document_type: None,
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: None,
            paper_meta: Some(PaperMetaInput {
                title: "2025高一数学期中".into(),
                year: Some(2025),
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
        assert!(n.is_paper && n.create_paper);
    }

    #[test]
    fn test_validate_confirm_paper_without_create() {
        let req = ConfirmDocumentRequest {
            source_category: Some("paper".into()),
            source_kind: Some("mock".into()),
            create_paper: Some(false),
            document_type: None,
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: Some("一模".into()),
            paper_meta: Some(PaperMetaInput {
                title: "".into(),
                year: None,
                stage: None,
                grade: None,
                subject: None,
                semester: None,
                region_province: None,
                region_city: None,
                school_name: None,
                source_type: None,
                sub_source_type: Some("一模".into()),
                paper_id: None,
            }),
            collections: None,
        };
        let n = validate_confirm(&req).unwrap();
        assert!(!n.create_paper);
        assert!(!n.is_paper);
    }

    #[test]
    fn test_validate_confirm_legacy_exam_compat() {
        let req = ConfirmDocumentRequest {
            source_category: None,
            source_kind: None,
            create_paper: Some(true),
            document_type: Some("exam".into()),
            type_label: None,
            title: None,
            source_type: None,
            sub_source_type: None,
            paper_meta: Some(PaperMetaInput {
                title: "卷".into(),
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
        // create_paper + legacy exam → mapped to paper, create_paper true
        // but resolve maps exam→paper/monthly_test; create_paper only if category==paper
        let n = validate_confirm(&req).unwrap();
        assert_eq!(n.source_category, "paper");
        assert!(n.create_paper);
    }

    #[test]
    fn test_should_create_paper() {
        assert!(should_create_paper("paper", true));
        assert!(!should_create_paper("paper", false));
        assert!(!should_create_paper("practice", true));
    }

    #[test]
    fn test_resolved_source_type_fallback() {
        let mut m = PaperMetaInput {
            title: "卷".into(),
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
        };
        assert_eq!(m.resolved_source_type("final").as_deref(), Some("final"));
        m.source_type = Some("midterm".into());
        assert_eq!(m.resolved_source_type("final").as_deref(), Some("midterm"));
    }
}
