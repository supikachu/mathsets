//! 导出引擎中间表示（IR）— 任务 T1.2
//!
//! 两层 IR 的第一层：`ExamBundle`（导出域：内容与语义，见实施计划 §5.1）。
//! 请求契约 `ExamRequest` 为 markdown / docx / pdf 三格式共用（§四）。
//! 所有类型经 ts-rs 导出 TypeScript 绑定至 `frontend/src/api/types/exam.ts`（B6），
//! 保证前后端字段一致；`QuestionKind` 必须含 `Composite`（§7.4，对应前端 bucketType 第 5 桶）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::models::question_structure::{AnalysisBlock, QuestionPart};
use crate::typeset::spec::LayoutSpec;

// ═══════════════════════════ 请求契约（ExamRequest） ═══════════════════════════

/// 导出请求（三格式共用；sections 由前端 groupedSections + displayNoMap 序列化，
/// 保留用户排序选择）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct ExamRequest {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub exam_meta: ExamMeta,
    pub mode: ExportMode,
    pub sections: Vec<ExamSectionRequest>,
    #[serde(default)]
    pub options: ExportOptions,
    /// 版面参数（T3.2）：缺省时 `/export/pdf` 按 mode 取内置预设，带了就整体替换（T3.3）
    #[serde(default)]
    #[ts(optional)]
    pub spec: Option<LayoutSpec>,
}

/// 导出模式：student=学生卷 / teacher=讲义（内嵌 Callout）/ exam=考卷（卷末答案）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub enum ExportMode {
    Student,
    Teacher,
    Exam,
}

/// 考试元信息（卷头）
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct ExamMeta {
    #[serde(default)]
    #[ts(optional)]
    pub school: Option<String>,
    /// 考试时长（分钟）
    #[serde(default)]
    #[ts(optional)]
    pub duration: Option<u32>,
    /// 总分
    #[serde(default)]
    #[ts(optional)]
    pub total_score: Option<f64>,
    /// 考试说明（逐条渲染）
    #[serde(default)]
    pub instructions: Vec<String>,
}

/// 请求中的大题（前端分组序列化）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct ExamSectionRequest {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub instruction: Option<String>,
    pub questions: Vec<ExamQuestionRequest>,
}

/// 请求中的题目引用（后端按此批量取题并重排连续题号）
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct ExamQuestionRequest {
    pub id: Uuid,
    /// 缺省时按后端默认值
    #[serde(default)]
    #[ts(optional)]
    pub default_score: Option<f64>,
}

// ═══════════════════════════ 导出选项（ExportOptions） ═══════════════════════════

/// 导出内容开关（§四 options）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct ExportOptions {
    /// 是否包含答案
    #[serde(default = "default_true")]
    pub include_answer: bool,
    /// 是否包含解析
    #[serde(default)]
    pub include_analysis: bool,
    /// 答案内嵌（false）还是卷末汇总（true）
    #[serde(default = "default_true")]
    pub answer_at_end: bool,
    #[serde(default)]
    pub callouts: CalloutOptions,
    /// 答题留白（B5：管开关与高度；None = 不留白）
    #[serde(default)]
    #[ts(optional)]
    pub answer_space: Option<AnswerSpace>,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            include_answer: true,
            include_analysis: false,
            answer_at_end: true,
            callouts: CalloutOptions::default(),
            answer_space: None,
        }
    }
}

/// Callout 开关（教师/讲义模式生效）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct CalloutOptions {
    #[serde(default = "default_true")]
    pub knowledge: bool,
    #[serde(default = "default_true")]
    pub error_prone: bool,
    #[serde(default = "default_true")]
    pub analysis: bool,
}

impl Default for CalloutOptions {
    fn default() -> Self {
        Self {
            knowledge: true,
            error_prone: true,
            analysis: true,
        }
    }
}

/// 答题留白配置（高度由 options 管、样式细节由 spec 管，冲突以 options 为准 — B5）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct AnswerSpace {
    pub style: BlankStyle,
    /// 留白高度（cm）
    #[serde(default = "default_blank_height_cm")]
    pub height_cm: f64,
}

/// 留白样式：横线格 / 点阵 / 纯空白
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub enum BlankStyle {
    Lines,
    Dots,
    Blank,
}

fn default_true() -> bool {
    true
}

fn default_blank_height_cm() -> f64 {
    6.0
}

// ═══════════════════════════ 装配后 IR（ExamBundle） ═══════════════════════════

/// 装配完成的试卷包（导出域 IR；三种生成器共用的唯一输入）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct ExamBundle {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub subtitle: Option<String>,
    pub exam_meta: ExamMeta,
    pub mode: ExportMode,
    pub sections: Vec<ExamSection>,
}

/// 大题（装配后）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct ExamSection {
    pub title: String,
    #[serde(default)]
    #[ts(optional)]
    pub instruction: Option<String>,
    pub questions: Vec<ExamQuestion>,
}

/// 题块（装配后）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct ExamQuestion {
    /// 后端按请求顺序重排的连续题号（从 1 起、跨大题连续）
    pub number: u32,
    pub score: f64,
    pub kind: QuestionKind,
    /// 题干（已切分为 InlineNode 序列）
    pub stem: Vec<InlineNode>,
    /// 选项（选择/多选题；其余题型为空）
    pub options: Vec<ExamOption>,
    /// 答案（多选字母 / 多空逐空；选择题为字母串）
    pub answers: Vec<String>,
    /// 名师点拨等解法块（复用问树 AnalysisBlock；content 为原始文本，生成期切分）
    pub analyses: Vec<AnalysisBlock>,
    /// 解答题问树（复用 QuestionPart；stem/answer/analyses 为原始文本，生成期切分）
    #[serde(default)]
    pub structure_parts: Vec<QuestionPart>,
    /// 教师模式派生的四类提示框
    #[serde(default)]
    pub callouts: Vec<Callout>,
    /// 本题留白覆盖（None = 沿用请求级 options.answer_space）
    #[serde(default)]
    #[ts(optional)]
    pub answer_space: Option<AnswerSpace>,
    /// 本题装配/生成期问题（公式降级、图片跳过等）
    #[serde(default)]
    pub issues: Vec<Issue>,
}

/// 题型（§7.4：Composite 综合题对应前端 bucketType 第 5 桶，不可遗漏）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub enum QuestionKind {
    SingleChoice,
    MultiChoice,
    Fill,
    Solution,
    Composite,
}

/// 选项（content 已切分为 InlineNode）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct ExamOption {
    /// 选项字母（A/B/C/D…）
    pub label: String,
    pub content: Vec<InlineNode>,
}

// ═══════════════════════════ Callout 与 Issue ═══════════════════════════

/// 教师模式提示框（§5.1：考点/易错/点拨/思路四类，统一挂题块）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct Callout {
    pub kind: CalloutKind,
    pub title: String,
    pub nodes: Vec<InlineNode>,
}

/// Callout 类别（四色框）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub enum CalloutKind {
    /// 考点清单（knowledge_nodes，主知识点置顶）
    Knowledge,
    /// 易错警示（error_prone 类标签）
    ErrorProne,
    /// 名师点拨（analysis）
    Tip,
    /// 思路拆解（问树 analyses 各解法块）
    Approach,
}

/// 装配/生成期问题（X-Export-Warnings 头与预检报告共用载体）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct Issue {
    /// 关联题号（卷级问题为 None）
    #[serde(default)]
    #[ts(optional)]
    pub question_no: Option<u32>,
    pub field: IssueField,
    pub severity: IssueSeverity,
    /// 公式降级时的原始 LaTeX（其余场景为 None）
    #[serde(default)]
    #[ts(optional)]
    pub latex: Option<String>,
    pub reason: String,
}

/// 问题所在字段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub enum IssueField {
    Stem,
    Analysis,
    Choice,
    Answer,
    Structure,
    Image,
    Other,
}

/// 问题级别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub enum IssueSeverity {
    Info,
    Warning,
    Error,
}

// ═══════════════════════════ 行内节点（InlineNode） ═══════════════════════════

/// 内容切分器输出（§5.2：对 stem/analysis/选项/问树文本一次性扫描）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub enum InlineNode {
    /// 纯文本段
    Text {
        text: String,
    },
    /// 换行（段落内 \n）
    LineBreak,
    /// 数学公式：$...$ / \(...\)（行内）与 $$...$$ / \[...\]（块级）
    Math {
        latex: String,
        display: bool,
    },
    /// 块级单图：![alt](url){width,align}
    Image {
        #[serde(default)]
        #[ts(optional)]
        alt: Option<String>,
        url: String,
        /// 宽度（px，编辑器语法原值；导出期按目标单位换算，docx 上限 14cm）
        #[serde(default)]
        #[ts(optional)]
        width: Option<u32>,
        #[serde(default)]
        #[ts(optional)]
        align: Option<ImageAlign>,
    },
    /// 并排图组：:::img-row {...} 围栏（align 仅作用于容器，图组内单图只有 width）
    ImgRow {
        #[serde(default)]
        #[ts(optional)]
        align: Option<ImageAlign>,
        images: Vec<InlineImage>,
        /// 图注行（围栏内图片下方的说明文字）
        #[serde(default)]
        #[ts(optional)]
        caption: Option<String>,
    },
    /// Markdown 管道表格（单元格内容为原始文本，生成期二次处理）
    Table {
        header: Vec<String>,
        /// 各列对齐（与 header 等长；缺省左对齐）
        #[serde(default)]
        aligns: Vec<TableAlign>,
        rows: Vec<Vec<String>>,
    },
}

/// 图组内单图（无 align —— 对齐作用于 img-row 容器整体）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub struct InlineImage {
    #[serde(default)]
    #[ts(optional)]
    pub alt: Option<String>,
    pub url: String,
    #[serde(default)]
    #[ts(optional)]
    pub width: Option<u32>,
}

/// 图片对齐
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub enum ImageAlign {
    Left,
    Center,
    Right,
}

/// 表格列对齐
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/exam.ts")]
pub enum TableAlign {
    Left,
    Center,
    Right,
}

// ═══════════════════════════ 测试 ═══════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    /// 实施计划 §四 的请求体样例必须能无损反序列化（serde 契约）
    #[test]
    fn exam_request_parses_plan_contract() {
        let json = r##"{
            "title": "2026 学年第一学期期中考试",
            "subtitle": "数学试卷",
            "exam_meta": {
                "school": "实验中学",
                "duration": 120,
                "total_score": 150,
                "instructions": ["答题前请填写姓名与考号"]
            },
            "mode": "exam",
            "sections": [
                {
                    "title": "一、单选题",
                    "instruction": "每题 5 分，共 40 分",
                    "questions": [
                        { "id": "0b7b8f24-0000-4000-8000-000000000001", "default_score": 5 }
                    ]
                }
            ],
            "options": {
                "include_answer": true,
                "include_analysis": false,
                "answer_at_end": true,
                "callouts": { "knowledge": true, "error_prone": true, "analysis": true },
                "answer_space": { "style": "lines", "height_cm": 6 }
            }
        }"##;
        let req: ExamRequest = serde_json::from_str(json).expect("plan contract JSON must parse");
        assert_eq!(req.mode, ExportMode::Exam);
        assert_eq!(req.subtitle.as_deref(), Some("数学试卷"));
        assert_eq!(req.exam_meta.school.as_deref(), Some("实验中学"));
        assert_eq!(req.exam_meta.duration, Some(120));
        assert_eq!(req.exam_meta.total_score, Some(150.0));
        assert_eq!(req.sections.len(), 1);
        assert_eq!(req.sections[0].questions[0].default_score, Some(5.0));
        let space = req.options.answer_space.expect("answer_space present");
        assert_eq!(space.style, BlankStyle::Lines);
        assert_eq!(space.height_cm, 6.0);
        // spec 可省略
        assert!(req.spec.is_none());
    }

    /// options / exam_meta / subtitle 均可整体缺省（serde default 链）
    #[test]
    fn exam_request_minimal_fields() {
        let json = r##"{
            "title": "练习",
            "mode": "student",
            "sections": [
                { "title": "一、填空题", "questions": [ { "id": "0b7b8f24-0000-4000-8000-000000000002" } ] }
            ]
        }"##;
        let req: ExamRequest = serde_json::from_str(json).expect("minimal JSON must parse");
        assert_eq!(req.mode, ExportMode::Student);
        assert!(req.subtitle.is_none());
        assert_eq!(req.exam_meta, ExamMeta::default());
        // 默认选项：含答案 / 不含解析 / 卷末汇总 / 无留白
        assert!(req.options.include_answer);
        assert!(!req.options.include_analysis);
        assert!(req.options.answer_at_end);
        assert!(req.options.answer_space.is_none());
        // 题目 default_score 缺省
        assert!(req.sections[0].questions[0].default_score.is_none());
    }

    /// QuestionKind 必须含 Composite（§7.4 DoD）
    #[test]
    fn question_kind_includes_composite() {
        let all = serde_json::to_value([
            QuestionKind::SingleChoice,
            QuestionKind::MultiChoice,
            QuestionKind::Fill,
            QuestionKind::Solution,
            QuestionKind::Composite,
        ])
        .unwrap();
        assert_eq!(
            all,
            serde_json::json!(["single_choice", "multi_choice", "fill", "solution", "composite"])
        );
    }

    /// InlineNode 内部标签序列化：{"type":"math",...}
    #[test]
    fn inline_node_tagged_serde() {
        let math = InlineNode::Math {
            latex: r"\frac{1}{2}".into(),
            display: false,
        };
        let v = serde_json::to_value(&math).unwrap();
        assert_eq!(v["type"], "math");
        assert_eq!(v["latex"], r"\frac{1}{2}");
        assert_eq!(v["display"], false);
        let back: InlineNode = serde_json::from_value(v).unwrap();
        assert_eq!(back, math);

        // 块级单图带配置
        let img = InlineNode::Image {
            alt: Some("图1".into()),
            url: "/uploads/questions/a.png".into(),
            width: Some(300),
            align: Some(ImageAlign::Center),
        };
        let v = serde_json::to_value(&img).unwrap();
        assert_eq!(v["type"], "image");
        assert_eq!(v["align"], "center");
        let back: InlineNode = serde_json::from_value(v).unwrap();
        assert_eq!(back, img);

        // 换行 / 文本
        assert_eq!(
            serde_json::to_value(InlineNode::LineBreak).unwrap()["type"],
            "line_break"
        );
        let text: InlineNode =
            serde_json::from_value(serde_json::json!({"type": "text", "text": "解："})).unwrap();
        assert_eq!(text, InlineNode::Text { text: "解：".into() });
    }

    /// 管道表格 / 图组节点序列化
    #[test]
    fn table_and_img_row_serde() {
        let table = InlineNode::Table {
            header: vec!["x".into(), "y".into()],
            aligns: vec![TableAlign::Center, TableAlign::Right],
            rows: vec![vec!["1".into(), "2".into()]],
        };
        let v = serde_json::to_value(&table).unwrap();
        assert_eq!(v["type"], "table");
        assert_eq!(v["aligns"], serde_json::json!(["center", "right"]));
        let back: InlineNode = serde_json::from_value(v).unwrap();
        assert_eq!(back, table);

        let row = InlineNode::ImgRow {
            align: Some(ImageAlign::Center),
            images: vec![InlineImage {
                alt: None,
                url: "/uploads/questions/b.png".into(),
                width: Some(200),
            }],
            caption: Some("图 1-1 与图 1-2".into()),
        };
        let v = serde_json::to_value(&row).unwrap();
        assert_eq!(v["type"], "img_row");
        assert_eq!(v["images"][0]["url"], "/uploads/questions/b.png");
        let back: InlineNode = serde_json::from_value(v).unwrap();
        assert_eq!(back, row);
    }

    /// ExamBundle 装配后 IR 往返（含问树复用与 Callout 派生形态）
    #[test]
    fn exam_bundle_roundtrip() {
        let bundle = ExamBundle {
            title: "测试卷".into(),
            subtitle: None,
            exam_meta: ExamMeta {
                school: Some("某某中学".into()),
                duration: Some(90),
                total_score: Some(120.0),
                instructions: vec!["闭卷".into()],
            },
            mode: ExportMode::Teacher,
            sections: vec![ExamSection {
                title: "一、解答题".into(),
                instruction: None,
                questions: vec![ExamQuestion {
                    number: 1,
                    score: 12.0,
                    kind: QuestionKind::Solution,
                    stem: vec![
                        InlineNode::Text { text: "已知函数 ".into() },
                        InlineNode::Math { latex: "f(x)=x^2".into(), display: false },
                    ],
                    options: vec![],
                    answers: vec!["x=1".into()],
                    analyses: vec![],
                    structure_parts: vec![QuestionPart {
                        id: "p1".into(),
                        label: "(1)".into(),
                        stem: "求导".into(),
                        children: vec![],
                        answer: Some("2x".into()),
                        analyses: vec![AnalysisBlock {
                            id: "a1".into(),
                            title: "解法一".into(),
                            content: "直接求导".into(),
                        }],
                        no_analysis_needed: false,
                        label_dirty: false,
                    }],
                    callouts: vec![Callout {
                        kind: CalloutKind::Knowledge,
                        title: "考点".into(),
                        nodes: vec![InlineNode::Text { text: "导数".into() }],
                    }],
                    answer_space: Some(AnswerSpace {
                        style: BlankStyle::Dots,
                        height_cm: 4.0,
                    }),
                    issues: vec![Issue {
                        question_no: Some(1),
                        field: IssueField::Stem,
                        severity: IssueSeverity::Warning,
                        latex: Some(r"\badcmd".into()),
                        reason: "公式转换失败，降级为原文".into(),
                    }],
                }],
            }],
        };
        let v = serde_json::to_value(&bundle).unwrap();
        let back: ExamBundle = serde_json::from_value(v).unwrap();
        assert_eq!(back, bundle);
        // 抽查关键 JSON 形态（与 X-Export-Warnings 契约对齐）
        let issue = &back.sections[0].questions[0].issues[0];
        assert_eq!(
            serde_json::to_value(&issue.field).unwrap(),
            serde_json::json!("stem")
        );
        assert_eq!(
            serde_json::to_value(&back.sections[0].questions[0].kind).unwrap(),
            serde_json::json!("solution")
        );
    }

    /// Issue 序列化为警告头契约形态 {question_no, field, latex, reason}
    #[test]
    fn issue_warning_contract() {
        let issue = Issue {
            question_no: Some(3),
            field: IssueField::Analysis,
            severity: IssueSeverity::Warning,
            latex: Some(r"\sqrt[3]".into()),
            reason: "非法 LaTeX".into(),
        };
        let v = serde_json::to_value(&issue).unwrap();
        assert_eq!(v["question_no"], 3);
        assert_eq!(v["field"], "analysis");
        assert_eq!(v["latex"], r"\sqrt[3]");
        assert_eq!(v["reason"], "非法 LaTeX");
        // 卷级问题（无题号）
        let global = Issue {
            question_no: None,
            field: IssueField::Image,
            severity: IssueSeverity::Info,
            latex: None,
            reason: "SVG 跳过".into(),
        };
        let v = serde_json::to_value(&global).unwrap();
        assert!(v.get("question_no").is_none() || v["question_no"].is_null());
    }
}
