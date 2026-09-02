//! Markdown 生成器（T1.6）— 实施计划 §5.5
//!
//! `ExamBundle + ExportOptions` → Markdown 文本（+ 可选 bundle zip）。
//!
//! - YAML frontmatter（题名 / 模式 / 总分）→ 卷头元信息 → `## 大题`（题数分值统计）
//!   → 题干 → 选项 → 问树 → Callout → 按开关内嵌或卷末汇总答案/解析。
//! - 公式 `$...$` / `$$...$$` **原样保留**；文本段做最小转义（`*` `_` `` ` `` `#`）。
//! - Callout 用 `> [!NOTE]` 风格引用块（GitHub alerts 语义映射四类）。
//! - `bundle=true`：外链/本地图片经抓取器拉取后入 `images/`，md 内 URL 重写。
//!   抓取失败降级为警告（不中断整卷），md 保留原 URL。

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::export::assets::{fetch_image, FetchImageError};
use crate::export::content::split_content;
use crate::export::model::{
    Callout, CalloutKind, ExamBundle, ExamQuestion, ExportMode, ExportOptions, InlineNode,
    Issue, IssueField, IssueSeverity, QuestionKind, TableAlign,
};
use crate::models::question_structure::{walk_leaves, QuestionPart};

/// 生成结果
pub struct MarkdownResult {
    /// Markdown 正文（bundle=false 时即下载内容）
    pub markdown: String,
    /// bundle=true 时的 zip 包（exam.md + images/）
    pub zip: Option<Vec<u8>>,
    /// 生成期新问题（图片抓取失败等；与题级 issues 合并进 X-Export-Warnings）
    pub issues: Vec<Issue>,
}

/// 生成 Markdown；图片抓取失败不中断（降级记警告）。
pub async fn generate_markdown(
    bundle: &ExamBundle,
    options: &ExportOptions,
    upload_dir: &Path,
    make_zip: bool,
) -> MarkdownResult {
    let mut issues: Vec<Issue> = Vec::new();
    let mut img_map: HashMap<String, String> = HashMap::new();
    let mut images: Vec<(String, Vec<u8>)> = Vec::new();

    if make_zip {
        let urls = collect_bundle_images(bundle, options);
        for (qno, url) in urls {
            if img_map.contains_key(&url) {
                continue;
            }
            match fetch_image(&url, upload_dir).await {
                Ok(img) => {
                    let name = format!("{}.{}", short_hash(&url), img.ext);
                    img_map.insert(url, format!("images/{}", name));
                    images.push((name, img.bytes));
                }
                Err(e) => issues.push(image_issue(qno, &url, &e)),
            }
        }
    }

    let markdown = render_markdown(bundle, options, &img_map);
    let zip = if make_zip {
        Some(build_zip(&markdown, &images))
    } else {
        None
    };
    MarkdownResult {
        markdown,
        zip,
        issues,
    }
}

fn image_issue(qno: Option<u32>, url: &str, e: &FetchImageError) -> Issue {
    Issue {
        question_no: qno,
        field: IssueField::Image,
        severity: IssueSeverity::Warning,
        latex: None,
        reason: format!("图片 {} 处理失败：{}", url, e),
    }
}

fn short_hash(s: &str) -> String {
    let digest = Sha256::digest(s.as_bytes());
    digest[..6].iter().map(|b| format!("{:02x}", b)).collect()
}

fn build_zip(markdown: &str, images: &[(String, Vec<u8>)]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    // start_file 返 ZipError 而 write_all 返 io::Error，错误类型不同不可 and_then 串接
    w.start_file("exam.md", opts).expect("start exam.md");
    w.write_all(markdown.as_bytes()).expect("write exam.md");
    for (name, bytes) in images {
        w.start_file(format!("images/{}", name), opts)
            .unwrap_or_else(|e| panic!("start image {}: {}", name, e));
        w.write_all(bytes)
            .unwrap_or_else(|e| panic!("write image {}: {}", name, e));
    }
    w.finish().expect("finish zip").into_inner()
}

// ═══════════════════════════ 图片 URL 收集 ═══════════════════════════

/// 收集全卷图片 URL（含问树/解析原始文本切分后的结果），带关联题号
///
/// 收集范围与 `render_markdown` 的门控保持一致：解析配图仅在 include_analysis 时打包，
/// 否则学生用卷的 zip 会夹带仅教师可见的解析图片。
///
/// `pub(crate)`：docx writer 复用同一份门控，两种格式不得打包出不同的图片集合。
pub(crate) fn collect_bundle_images(
    bundle: &ExamBundle,
    options: &ExportOptions,
) -> Vec<(Option<u32>, String)> {
    let mut out = Vec::new();
    for sec in &bundle.sections {
        for q in &sec.questions {
            collect_question_images(q, options, &mut out);
        }
    }
    out
}

fn collect_question_images(
    q: &ExamQuestion,
    options: &ExportOptions,
    out: &mut Vec<(Option<u32>, String)>,
) {
    let qno = Some(q.number);
    collect_inline_images(&q.stem, qno, out);
    for opt in &q.options {
        collect_inline_images(&opt.content, qno, out);
    }
    for c in &q.callouts {
        collect_inline_images(&c.nodes, qno, out);
    }
    if options.include_analysis {
        for blk in &q.analyses {
            collect_inline_images(&split_content(&blk.content), qno, out);
        }
    }
    collect_part_images(&q.structure_parts, options, qno, out);
}

fn collect_part_images(
    parts: &[QuestionPart],
    options: &ExportOptions,
    qno: Option<u32>,
    out: &mut Vec<(Option<u32>, String)>,
) {
    for p in parts {
        collect_inline_images(&split_content(&p.stem), qno, out);
        if let Some(a) = p
            .answer
            .as_deref()
            .filter(|a| options.include_answer && !a.trim().is_empty())
        {
            collect_inline_images(&split_content(a), qno, out);
        }
        if options.include_analysis {
            for blk in &p.analyses {
                collect_inline_images(&split_content(&blk.content), qno, out);
            }
        }
        collect_part_images(&p.children, options, qno, out);
    }
}

fn collect_inline_images(nodes: &[InlineNode], qno: Option<u32>, out: &mut Vec<(Option<u32>, String)>) {
    for n in nodes {
        match n {
            InlineNode::Image { url, .. } => out.push((qno, url.clone())),
            InlineNode::ImgRow { images, .. } => {
                for img in images {
                    out.push((qno, img.url.clone()));
                }
            }
            _ => {}
        }
    }
}

// ═══════════════════════════ 渲染 ═══════════════════════════

fn render_markdown(
    bundle: &ExamBundle,
    options: &ExportOptions,
    img_map: &HashMap<String, String>,
) -> String {
    let mut md = String::new();

    // ── frontmatter ──
    let total = bundle
        .exam_meta
        .total_score
        .unwrap_or_else(|| bundle.sections.iter().flat_map(|s| s.questions.iter()).map(|q| q.score).sum());
    md.push_str("---\n");
    md.push_str(&format!("title: \"{}\"\n", escape_yaml(&bundle.title)));
    if let Some(s) = &bundle.subtitle {
        md.push_str(&format!("subtitle: \"{}\"\n", escape_yaml(s)));
    }
    if let Some(s) = &bundle.exam_meta.school {
        md.push_str(&format!("school: \"{}\"\n", escape_yaml(s)));
    }
    if let Some(d) = bundle.exam_meta.duration {
        md.push_str(&format!("duration: {}\n", d));
    }
    md.push_str(&format!("mode: {}\n", mode_str(bundle.mode)));
    md.push_str(&format!("total_score: {}\n", fmt_score(total)));
    md.push_str("---\n\n");

    // ── 卷头 ──
    md.push_str(&format!("# {}\n\n", escape_md(&bundle.title)));
    if let Some(s) = &bundle.subtitle {
        md.push_str(&format!("{}\n\n", escape_md(s)));
    }
    let mut meta_parts: Vec<String> = Vec::new();
    if let Some(s) = &bundle.exam_meta.school {
        meta_parts.push(escape_md(s));
    }
    if let Some(d) = bundle.exam_meta.duration {
        meta_parts.push(format!("考试时长 {} 分钟", d));
    }
    meta_parts.push(format!("总分 {} 分", fmt_score(total)));
    md.push_str(&format!("{}\n\n", meta_parts.join(" · ")));
    if !bundle.exam_meta.instructions.is_empty() {
        md.push_str("**考试说明**\n\n");
        for ins in &bundle.exam_meta.instructions {
            md.push_str(&format!("- {}\n", escape_md(ins)));
        }
        md.push('\n');
    }

    // ── 大题 ──
    for sec in &bundle.sections {
        let count = sec.questions.len();
        let score: f64 = sec.questions.iter().map(|q| q.score).sum();
        md.push_str(&format!(
            "## {}（共 {} 题 · {} 分）\n\n",
            escape_md(&sec.title),
            count,
            fmt_score(score)
        ));
        if let Some(ins) = &sec.instruction {
            md.push_str(&format!("> {}\n\n", escape_md(ins)));
        }
        for q in &sec.questions {
            render_question(&mut md, q, options, img_map);
        }
    }

    // ── 卷末汇总 ──
    if options.include_answer && options.answer_at_end {
        md.push_str("## 参考答案\n\n");
        for sec in &bundle.sections {
            for q in &sec.questions {
                if answer_is_empty(q) {
                    continue;
                }
                md.push_str(&format!("{}. {}\n", q.number, render_answer_text(q)));
            }
        }
        md.push('\n');
    }
    if options.include_analysis && options.answer_at_end {
        md.push_str("## 试题解析\n\n");
        for sec in &bundle.sections {
            for q in &sec.questions {
                if let Some(a) = render_analysis(q, img_map) {
                    md.push_str(&format!("**{}.**\n\n{}\n", q.number, a));
                }
            }
        }
    }

    md
}

fn render_question(
    md: &mut String,
    q: &ExamQuestion,
    options: &ExportOptions,
    img_map: &HashMap<String, String>,
) {
    md.push_str(&format!("**{}.**（{} 分）", q.number, fmt_score(q.score)));
    md.push_str(&render_inline(&q.stem, img_map));
    md.push_str("\n\n");

    if !q.options.is_empty() {
        for opt in &q.options {
            let content = render_inline(&opt.content, img_map);
            if opt.label.is_empty() {
                md.push_str(&format!("{}\n", content));
            } else {
                md.push_str(&format!("{}. {}\n", escape_md(&opt.label), content));
            }
        }
        md.push('\n');
    }

    if !q.structure_parts.is_empty() {
        md.push_str(&render_parts(&q.structure_parts, img_map));
        md.push('\n');
    }

    for c in &q.callouts {
        render_callout(md, c, img_map);
    }

    if options.include_answer && !options.answer_at_end && !answer_is_empty(q) {
        md.push_str(&format!("**答案**：{}\n\n", render_answer_text(q)));
    }
    if options.include_analysis && !options.answer_at_end {
        if let Some(a) = render_analysis(q, img_map) {
            md.push_str(&format!("**解析**：\n\n{}\n", a));
        }
    }
}

/// 问树渲染：label + stem 逐层展开（叶子答案/解析由答案区或解析区管）
fn render_parts(parts: &[QuestionPart], img_map: &HashMap<String, String>) -> String {
    let mut out = String::new();
    for p in parts {
        let stem = render_inline(&split_content(&p.stem), img_map);
        if p.label.is_empty() {
            out.push_str(&stem);
        } else {
            out.push_str(&format!("**{}** {}", escape_md(&p.label), stem));
        }
        out.push_str("\n\n");
        if !p.children.is_empty() {
            out.push_str(&render_parts(&p.children, img_map));
        }
    }
    out
}

fn render_callout(md: &mut String, c: &Callout, img_map: &HashMap<String, String>) {
    let alert = match c.kind {
        CalloutKind::Knowledge => "NOTE",
        CalloutKind::ErrorProne => "CAUTION",
        CalloutKind::Tip => "TIP",
        CalloutKind::Approach => "IMPORTANT",
    };
    md.push_str(&format!("> [{}] {}\n", alert, escape_md(&c.title)));
    let body = render_inline(&c.nodes, img_map);
    for line in body.lines() {
        md.push_str(&format!("> {}\n", line));
    }
    md.push('\n');
}

/// 答案文本：解答题按问树叶子（label + 答案），其余按空分隔
fn render_answer_text(q: &ExamQuestion) -> String {
    if q.kind == QuestionKind::Solution && !q.structure_parts.is_empty() {
        walk_leaves(&q.structure_parts)
            .iter()
            .filter_map(|p| {
                p.answer
                    .as_deref()
                    .map(|a| format!("{} {}", escape_md(&p.label), a.trim()))
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        q.answers.iter().map(|a| escape_md(a)).collect::<Vec<_>>().join("；")
    }
}

fn answer_is_empty(q: &ExamQuestion) -> bool {
    if q.kind == QuestionKind::Solution && !q.structure_parts.is_empty() {
        return walk_leaves(&q.structure_parts)
            .iter()
            .all(|p| p.answer.as_deref().map(str::trim).unwrap_or("").is_empty());
    }
    q.answers.is_empty()
}

/// 解析文本：题级 analyses 块 + 问树叶子各解法块（原始文本 → 切分渲染）
fn render_analysis(q: &ExamQuestion, img_map: &HashMap<String, String>) -> Option<String> {
    let mut out = String::new();
    for blk in &q.analyses {
        let content = blk.content.trim();
        if content.is_empty() {
            continue;
        }
        if !blk.title.trim().is_empty() {
            out.push_str(&format!("**{}**\n\n", escape_md(&blk.title)));
        }
        out.push_str(&render_inline(&split_content(content), img_map));
        out.push_str("\n\n");
    }
    for leaf in walk_leaves(&q.structure_parts) {
        for blk in &leaf.analyses {
            let content = blk.content.trim();
            if content.is_empty() {
                continue;
            }
            let title = if blk.title.trim().is_empty() {
                leaf.label.clone()
            } else if leaf.label.is_empty() {
                blk.title.clone()
            } else {
                format!("{} {}", leaf.label, blk.title)
            };
            out.push_str(&format!("**{}**\n\n", escape_md(&title)));
            out.push_str(&render_inline(&split_content(content), img_map));
            out.push_str("\n\n");
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

// ── 行内节点渲染 ──

fn render_inline(nodes: &[InlineNode], img_map: &HashMap<String, String>) -> String {
    let mut out = String::new();
    for n in nodes {
        match n {
            InlineNode::Text { text } => out.push_str(&escape_md(text)),
            InlineNode::LineBreak => out.push('\n'),
            InlineNode::Math { latex, display: false } => {
                out.push('$');
                out.push_str(latex); // 公式原样保留，不转义
                out.push('$');
            }
            InlineNode::Math { latex, display: true } => {
                ensure_block(&mut out);
                out.push_str("$$");
                out.push_str(latex);
                out.push_str("$$\n\n");
            }
            InlineNode::Image {
                alt,
                url,
                width,
                ..
            } => {
                ensure_block(&mut out);
                out.push_str(&render_image_md(alt.as_deref(), url, *width, img_map));
                out.push_str("\n\n");
            }
            InlineNode::ImgRow {
                images,
                caption,
                ..
            } => {
                ensure_block(&mut out);
                for img in images {
                    out.push_str(&render_image_md(img.alt.as_deref(), &img.url, img.width, img_map));
                    out.push('\n');
                }
                if let Some(c) = caption.as_deref().filter(|c| !c.trim().is_empty()) {
                    out.push_str(&format!("*{}*\n", escape_md(c)));
                }
                out.push('\n');
            }
            InlineNode::Table {
                header,
                aligns,
                rows,
            } => {
                ensure_block(&mut out);
                out.push_str(&render_table(header, aligns, rows));
                out.push_str("\n\n");
            }
        }
    }
    out.trim_end().to_string()
}

fn render_image_md(
    alt: Option<&str>,
    url: &str,
    width: Option<u32>,
    img_map: &HashMap<String, String>,
) -> String {
    let resolved = img_map.get(url).cloned().unwrap_or_else(|| url.to_string());
    match width {
        // width 需 HTML img 表达（Markdown 原生语法无宽高）
        Some(w) => format!(
            "<img src=\"{}\" alt=\"{}\" width=\"{}\">",
            escape_html_attr(&resolved),
            escape_html_attr(alt.unwrap_or("")),
            w
        ),
        None => format!(
            "![{}]({})",
            alt.map(escape_md).unwrap_or_default(),
            resolved
        ),
    }
}

fn render_table(header: &[String], aligns: &[TableAlign], rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    out.push('|');
    for h in header {
        out.push_str(&format!(" {} |", escape_cell(h)));
    }
    out.push_str("\n|");
    for (i, _) in header.iter().enumerate() {
        let align = aligns.get(i).copied().unwrap_or(TableAlign::Left);
        out.push_str(&format!(" {} |", match align {
            TableAlign::Left => ":---",
            TableAlign::Center => ":---:",
            TableAlign::Right => "---:",
        }));
    }
    out.push('\n');
    for row in rows {
        out.push('|');
        for cell in row {
            out.push_str(&format!(" {} |", escape_cell(cell)));
        }
        out.push('\n');
    }
    out
}

/// 表格单元格：先转义 `|`（防破坏列结构），再走普通转义
fn escape_cell(s: &str) -> String {
    escape_md(&s.replace('|', "\\|"))
}

/// 块级节点前确保空行分隔
fn ensure_block(out: &mut String) {
    if out.is_empty() {
        return;
    }
    if !out.ends_with("\n\n") {
        if out.ends_with('\n') {
            out.push('\n');
        } else {
            out.push_str("\n\n");
        }
    }
}

// ── 转义与小工具 ──

/// 文本段最小转义（公式段不转义）：`*` `_` `` ` `` `#`
fn escape_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '_' | '`' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn escape_yaml(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_html_attr(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn mode_str(m: ExportMode) -> &'static str {
    match m {
        ExportMode::Student => "student",
        ExportMode::Teacher => "teacher",
        ExportMode::Exam => "exam",
    }
}

/// 分数显示：整数不带小数点（docx writer 共用，两种格式的分值口径必须一致）
pub(crate) fn fmt_score(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{}", v as i64)
    } else {
        format!("{}", v)
    }
}

// ═══════════════════════════ 单元测试 ═══════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::model::{
        AnswerSpace, BlankStyle, ExamMeta, ExamOption, ExamSection, InlineImage, TableAlign,
    };
    use crate::models::question_structure::{AnalysisBlock, QuestionPart};
    use std::collections::HashMap;
    use std::io::Read;

    fn bundle_1q(mode: ExportMode, q: ExamQuestion) -> ExamBundle {
        ExamBundle {
            title: "集合单元测验".to_string(),
            subtitle: Some("数学".to_string()),
            exam_meta: ExamMeta {
                school: Some("实验中学".to_string()),
                duration: Some(90),
                total_score: None,
                instructions: vec!["闭卷作答".to_string()],
            },
            mode,
            sections: vec![ExamSection {
                title: "一、单选题".to_string(),
                instruction: Some("每题 5 分".to_string()),
                questions: vec![q],
            }],
        }
    }

    fn choice_question(number: u32, stem: Vec<InlineNode>) -> ExamQuestion {
        ExamQuestion {
            number,
            score: 5.0,
            kind: QuestionKind::SingleChoice,
            stem,
            options: vec![
                ExamOption {
                    label: "A".to_string(),
                    content: vec![InlineNode::Text { text: "空集没有子集".to_string() }],
                },
                ExamOption {
                    label: "B".to_string(),
                    content: vec![InlineNode::Text { text: "空集是任何集合的子集".to_string() }],
                },
            ],
            answers: vec!["B".to_string()],
            analyses: vec![AnalysisBlock {
                id: "analysis".to_string(),
                title: String::new(),
                content: "子集个数 $2^n$".to_string(),
            }],
            structure_parts: vec![],
            callouts: vec![],
            answer_space: None,
            issues: vec![],
        }
    }

    #[test]
    fn frontmatter_and_header() {
        let q = choice_question(1, vec![InlineNode::Text { text: "题干".to_string() }]);
        let b = bundle_1q(ExportMode::Exam, q);
        let md = render_markdown(&b, &ExportOptions::default(), &HashMap::new());

        assert!(md.starts_with("---\n"));
        assert!(md.contains("title: \"集合单元测验\""));
        assert!(md.contains("subtitle: \"数学\""));
        assert!(md.contains("school: \"实验中学\""));
        assert!(md.contains("duration: 90"));
        assert!(md.contains("mode: exam"));
        // 总分 = 题分求和（exam_meta.total_score 缺省）
        assert!(md.contains("total_score: 5"));
        assert!(md.contains("# 集合单元测验"));
        assert!(md.contains("实验中学 · 考试时长 90 分钟 · 总分 5 分"));
        assert!(md.contains("- 闭卷作答"));
    }

    #[test]
    fn section_stats_and_question() {
        let q = choice_question(1, vec![InlineNode::Text { text: "题干".to_string() }]);
        let b = bundle_1q(ExportMode::Student, q);
        let md = render_markdown(&b, &ExportOptions::default(), &HashMap::new());

        assert!(md.contains("## 一、单选题（共 1 题 · 5 分）"));
        assert!(md.contains("> 每题 5 分"));
        assert!(md.contains("**1.**（5 分）题干"));
        assert!(md.contains("A. 空集没有子集"));
        assert!(md.contains("B. 空集是任何集合的子集"));
    }

    #[test]
    fn math_preserved_and_text_escaped() {
        let stem = vec![
            InlineNode::Text { text: "已知 3 * 4 与 x_1，则".to_string() },
            InlineNode::Math { latex: "f(x)=x^2_1".to_string(), display: false },
            InlineNode::Text { text: "，且 `code` # 不转义公式内 * 号".to_string() },
            InlineNode::Math { latex: r"\frac{1}{2}".to_string(), display: true },
        ];
        let q = choice_question(1, stem);
        let b = bundle_1q(ExportMode::Student, q);
        let md = render_markdown(&b, &ExportOptions::default(), &HashMap::new());

        // 文本最小转义
        assert!(md.contains("3 \\* 4"));
        assert!(md.contains("x\\_1"));
        assert!(md.contains("\\`code\\`"));
        assert!(md.contains("\\#"));
        // 公式原样（内部 _ ^ * 不转义）
        assert!(md.contains("$f(x)=x^2_1$"));
        assert!(md.contains("$$\\frac{1}{2}$$"));
        assert!(!md.contains("x\\^"));
    }

    #[test]
    fn answer_inline_vs_at_end() {
        let q = choice_question(1, vec![InlineNode::Text { text: "题干".to_string() }]);
        let b = bundle_1q(ExportMode::Student, q);

        // 内嵌
        let mut opts = ExportOptions::default();
        opts.include_answer = true;
        opts.answer_at_end = false;
        opts.include_analysis = true;
        let md = render_markdown(&b, &opts, &HashMap::new());
        assert!(md.contains("**答案**：B"));
        assert!(md.contains("**解析**："));
        assert!(md.contains("子集个数 $2^n$"));
        assert!(!md.contains("## 参考答案"));

        // 卷末
        let mut opts = ExportOptions::default();
        opts.include_answer = true;
        opts.answer_at_end = true;
        opts.include_analysis = true;
        let md = render_markdown(&b, &opts, &HashMap::new());
        assert!(md.contains("## 参考答案"));
        assert!(md.contains("1. B"));
        assert!(md.contains("## 试题解析"));
        assert!(!md.contains("**答案**：B"));

        // 不含答案
        let mut opts = ExportOptions::default();
        opts.include_answer = false;
        let md = render_markdown(&b, &opts, &HashMap::new());
        assert!(!md.contains("参考答案"));
    }

    #[test]
    fn callout_github_alerts() {
        let mut q = choice_question(1, vec![InlineNode::Text { text: "题干".to_string() }]);
        q.callouts = vec![
            Callout {
                kind: CalloutKind::Knowledge,
                title: "考点清单".to_string(),
                nodes: vec![InlineNode::Text { text: "导数、二次函数".to_string() }],
            },
            Callout {
                kind: CalloutKind::ErrorProne,
                title: "易错警示".to_string(),
                nodes: vec![InlineNode::Text { text: "忽略定义域".to_string() }],
            },
            Callout {
                kind: CalloutKind::Tip,
                title: "名师点拨".to_string(),
                nodes: vec![InlineNode::Text { text: "先分类".to_string() }],
            },
            Callout {
                kind: CalloutKind::Approach,
                title: "解法一".to_string(),
                nodes: vec![InlineNode::Text { text: "数形结合".to_string() }],
            },
        ];
        let b = bundle_1q(ExportMode::Teacher, q);
        let md = render_markdown(&b, &ExportOptions::default(), &HashMap::new());
        assert!(md.contains("> [NOTE] 考点清单\n> 导数、二次函数"));
        assert!(md.contains("> [CAUTION] 易错警示\n> 忽略定义域"));
        assert!(md.contains("> [TIP] 名师点拨\n> 先分类"));
        assert!(md.contains("> [IMPORTANT] 解法一\n> 数形结合"));
    }

    #[test]
    fn solution_tree_answers_and_analysis() {
        let q = ExamQuestion {
            number: 2,
            score: 12.0,
            kind: QuestionKind::Solution,
            stem: vec![InlineNode::Text { text: "已知函数".to_string() }],
            options: vec![],
            answers: vec![],
            analyses: vec![],
            structure_parts: vec![QuestionPart {
                id: "p1".to_string(),
                label: "(1)".to_string(),
                stem: "求单调区间".to_string(),
                children: vec![QuestionPart {
                    id: "p1-1".to_string(),
                    label: "①".to_string(),
                    stem: "当 $x>0$ 时".to_string(),
                    children: vec![],
                    answer: Some("递增".to_string()),
                    analyses: vec![AnalysisBlock {
                        id: "a1".to_string(),
                        title: "解法一".to_string(),
                        content: "求导 $f'(x)>0$".to_string(),
                    }],
                    no_analysis_needed: false,
                    label_dirty: false,
                }],
                answer: None,
                analyses: vec![],
                no_analysis_needed: false,
                label_dirty: false,
            }],
            callouts: vec![],
            answer_space: None,
            issues: vec![],
        };
        let b = ExamBundle {
            title: "解答卷".to_string(),
            subtitle: None,
            exam_meta: ExamMeta::default(),
            mode: ExportMode::Student,
            sections: vec![ExamSection {
                title: "一、解答题".to_string(),
                instruction: None,
                questions: vec![q],
            }],
        };
        let mut opts = ExportOptions::default();
        opts.include_answer = true;
        opts.answer_at_end = true;
        opts.include_analysis = true;
        let md = render_markdown(&b, &opts, &HashMap::new());

        // 问树展开渲染（嵌套 label）
        assert!(md.contains("**(1)** 求单调区间"));
        assert!(md.contains("**①** 当 $x>0$ 时"));
        // 卷末答案：叶子 label + 答案
        assert!(md.contains("2. ① 递增"));
        // 解析：叶子解法块
        assert!(md.contains("**① 解法一**"));
        assert!(md.contains("求导 $f'(x)>0$"));
    }

    #[test]
    fn table_and_img_row_render() {
        let q = choice_question(
            1,
            vec![
                InlineNode::Table {
                    header: vec!["x".to_string(), "y".to_string()],
                    aligns: vec![TableAlign::Center, TableAlign::Right],
                    rows: vec![vec!["1".to_string(), "2|3".to_string()]],
                },
                InlineNode::ImgRow {
                    align: None,
                    images: vec![InlineImage {
                        alt: None,
                        url: "/uploads/questions/a.png".to_string(),
                        width: Some(200),
                    }],
                    caption: Some("图 1".to_string()),
                },
            ],
        );
        let b = bundle_1q(ExportMode::Student, q);
        let md = render_markdown(&b, &ExportOptions::default(), &HashMap::new());
        assert!(md.contains("| x | y |"));
        assert!(md.contains("| :---: | ---: |"));
        // 单元格内 | 转义
        assert!(md.contains("| 1 | 2\\|3 |"));
        assert!(md.contains("<img src=\"/uploads/questions/a.png\" alt=\"\" width=\"200\">"));
        assert!(md.contains("*图 1*"));
    }

    #[tokio::test]
    async fn bundle_zip_with_local_image_and_missing_fallback() {
        // 本地图片命中 → 入 zip；缺失图片 → 警告 + 保留原 URL
        let dir = std::env::temp_dir().join(format!("mathset-md-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("questions")).unwrap();
        let png: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        std::fs::write(dir.join("questions/a.png"), png).unwrap();

        let q = choice_question(
            1,
            vec![
                InlineNode::Image {
                    alt: Some("图A".to_string()),
                    url: "/uploads/questions/a.png".to_string(),
                    width: None,
                    align: None,
                },
                InlineNode::Image {
                    alt: None,
                    url: "/uploads/questions/missing.png".to_string(),
                    width: None,
                    align: None,
                },
            ],
        );
        let b = bundle_1q(ExportMode::Student, q);
        let result = generate_markdown(&b, &ExportOptions::default(), &dir, true).await;

        // 缺失图片 → 一条 Image 警告
        assert_eq!(result.issues.len(), 1);
        assert_eq!(result.issues[0].field, IssueField::Image);
        assert_eq!(result.issues[0].question_no, Some(1));
        assert!(result.issues[0].reason.contains("missing.png"));

        // md 中：命中图重写为 images/…；缺失图保留原 URL
        assert!(result.markdown.contains("](images/"));
        assert!(result.markdown.contains("(/uploads/questions/missing.png)"));

        // zip 结构：exam.md + images/*.png
        let zip_bytes = result.zip.expect("zip present");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&zip_bytes)).unwrap();
        let names: Vec<String> = archive.file_names().map(String::from).collect();
        assert!(names.contains(&"exam.md".to_string()));
        assert_eq!(names.iter().filter(|n| n.starts_with("images/")).count(), 1);
        let mut entry = archive.by_name("exam.md").unwrap();
        let mut md_in_zip = String::new();
        entry.read_to_string(&mut md_in_zip).unwrap();
        assert_eq!(md_in_zip, result.markdown);

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn bundle_zip_fetches_remote_image() {
        // 外链图片拉取后入包（复用 assets 的 mini server 手法）
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let png: Vec<u8> = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { break };
                let body = png.clone();
                tokio::spawn(async move {
                    use tokio::io::{AsyncReadExt, AsyncWriteExt};
                    let mut buf = [0u8; 2048];
                    let _ = sock.read(&mut buf).await;
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = sock.write_all(head.as_bytes()).await;
                    let _ = sock.write_all(&body).await;
                });
            }
        });

        let q = choice_question(
            1,
            vec![InlineNode::Image {
                alt: None,
                url: format!("http://{}/img.png", addr),
                width: None,
                align: None,
            }],
        );
        let b = bundle_1q(ExportMode::Student, q);
        let result = generate_markdown(
            &b,
            &ExportOptions::default(),
            Path::new("./uploads"),
            true,
        )
        .await;
        assert!(result.issues.is_empty());
        assert!(result.markdown.contains("](images/"));
        let zip_bytes = result.zip.unwrap();
        let archive = zip::ZipArchive::new(std::io::Cursor::new(&zip_bytes)).unwrap();
        assert_eq!(
            archive
                .file_names()
                .filter(|n| n.starts_with("images/") && n.ends_with(".png"))
                .count(),
            1
        );
    }

    #[test]
    fn bundle_images_follow_analysis_switch() {
        // 学生用卷打包时不得夹带仅教师可见的解析配图（内容泄漏 + 冗余）
        let mut q = choice_question(
            1,
            vec![InlineNode::Image {
                alt: None,
                url: "/uploads/questions/stem.png".to_string(),
                width: None,
                align: None,
            }],
        );
        q.analyses = vec![AnalysisBlock {
            id: "analysis".to_string(),
            title: String::new(),
            content: "![解析图](/uploads/questions/an.png)".to_string(),
        }];
        let b = bundle_1q(ExportMode::Student, q);

        let student: Vec<String> = collect_bundle_images(&b, &ExportOptions::default())
            .into_iter()
            .map(|(_, u)| u)
            .collect();
        assert_eq!(student, vec!["/uploads/questions/stem.png".to_string()]);

        let teacher = ExportOptions {
            include_analysis: true,
            ..ExportOptions::default()
        };
        let urls = collect_bundle_images(&b, &teacher);
        assert!(urls.iter().any(|(_, u)| u.ends_with("an.png")));
        assert_eq!(urls[0].0, Some(1));
    }

    #[test]
    fn fmt_score_integer() {
        assert_eq!(fmt_score(20.0), "20");
        assert_eq!(fmt_score(2.5), "2.5");
    }

    #[allow(dead_code)]
    fn answer_space_field_compiles(a: Option<AnswerSpace>) -> BlankStyle {
        // 保证 model 侧字段仍在 IR 内（未使用仅编译验证）
        a.map(|s| s.style).unwrap_or(BlankStyle::Blank)
    }
}
