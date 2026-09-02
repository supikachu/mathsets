//! ExamBundle → LayoutDoc 适配器（实施计划 §5.6）— 任务 T3.3
//!
//! **两个模块之间的唯一桥**：导出引擎把内容与语义装配成 `ExamBundle`，本文件把它翻译成排版域
//! 的 `LayoutDoc` + 定稿 `LayoutSpec`，`typeset::typst_gen` 只认后者。Markdown 与 DOCX 完全不
//! 经过排版系统，排版系统也不认得 `ExamBundle`。
//!
//! 适配器负责的四件事：
//! 1. **模式翻译**：`ExportMode → OutputProfile`，并按 profile 取内置版面预设；
//! 2. **选项栅格**：把 `LayoutSpec` 的栏宽（mm）换算成 em 后交给
//!    [`choice_grid::decide`]，与 docx `w:tbl` 共用同一份判定；
//! 3. **留白合并**（B5）：开关与高度在 `options.answer_space`（或题级覆盖）手里，样式在
//!    `spec.answer_blank` 手里，两者冲突以 options 为准并记一条 info；
//! 4. **文本切分**：问树 `stem` 与解析 `content` 在 bundle 里仍是原始文本，出口前一律过
//!    [`split_content`]，让 IR 只剩 `InlineNode`（见 [`crate::typeset::ir`] 的不变式 1）；
//! 5. **题型出块**（T4.1）：「这一题该出哪些块」在 [`crate::typeset::blocks`] 的注册表里，
//!    本文件只查表，然后把模式差异（Callout、内嵌答案）续在题面后面。
//!
//! T3.7 起本模块兼任 **PDF 渲染出口**（[`generate_pdf`]：预取素材 → `typst_gen` → `compiler`）。
//! 放在桥上而不是 handler 里，是因为「这张卷子要哪些图」只有排版域自己说得清，而 R1 已经把
//! PDF 出口唯一化到 `/export/pdf` —— 一条链、一个入口。

use std::collections::HashMap;
use std::path::Path;

use crate::export::assets::{FetchedImage, fetch_image};
use crate::export::content::split_content;
use crate::export::model::{
    BlankStyle as WireBlankStyle, ExamBundle, ExamQuestion, ExamSection, ExportMode, ExportOptions,
    InlineNode, Issue, IssueField, IssueSeverity,
};
use crate::models::question_structure::walk_leaves;
use crate::typeset::blocks::choice_grid;
use crate::typeset::blocks::{BlockCtx, Registry};
use crate::typeset::compiler::{
    CompileError, CompileRequest, compile_pdf, font_dirs, missing_cjk_fonts,
};
use crate::typeset::ir::{
    AnalysisEntry, AnswerBlock, AnswerLine, BlockMeta, CalloutBlock, DocumentMeta, LayoutBlock,
    LayoutDoc, Section, SectionHeader,
};
use crate::typeset::spec::{BlankStyle, LayoutSpec, OutputProfile};
use crate::typeset::typst_gen;

/// 题号「3.」的悬挂缩进占宽（em）—— docx 侧是 420tw = 2em，两处必须一致
const HANGING_EM: f64 = 2.0;

/// 排版域 IR 的入口：`ExamBundle` + 导出选项（+ 请求里的版面覆盖）→ `LayoutDoc`
pub fn build_layout_doc(
    bundle: &ExamBundle,
    options: &ExportOptions,
    request_spec: Option<&LayoutSpec>,
) -> LayoutDoc {
    layout_doc(bundle, options, request_spec, &Registry::standard())
}

/// 同上，但题型模板表由调用方给（T4.1 的扩展性入口；`build_layout_doc` 用内置五题型）
fn layout_doc(
    bundle: &ExamBundle,
    options: &ExportOptions,
    request_spec: Option<&LayoutSpec>,
    registry: &Registry,
) -> LayoutDoc {
    let profile = profile_of(bundle.mode);
    let spec = resolve_spec(profile, request_spec);
    let issues = blank_conflicts(options, &spec);
    let ctx = BlockCtx {
        options,
        spec: &spec,
        profile,
        available_em: available_em(&spec),
        registry,
    };

    let sections: Vec<Section> = bundle
        .sections
        .iter()
        .map(|sec| section(sec, &ctx))
        .collect();
    // 卷末答案区：答案与解析是两个独立开关，任一个开着就要走一遍（两者皆空的题由 answer_block 自己滤掉）
    let answer_key = if options.answer_at_end {
        bundle
            .sections
            .iter()
            .flat_map(|sec| sec.questions.iter())
            .filter_map(|q| answer_block(q, &ctx, BlockMeta::flow()))
            .collect()
    } else {
        Vec::new()
    };

    LayoutDoc {
        title: bundle.title.clone(),
        subtitle: bundle.subtitle.clone(),
        meta: document_meta(bundle),
        profile,
        spec,
        sections,
        answer_key,
        issues,
    }
}

// ═══════════════════════════ 模式与版面 ═══════════════════════════

/// `ExportMode → OutputProfile`（§5.6：三种模式各自的默认口径）
pub fn profile_of(mode: ExportMode) -> OutputProfile {
    match mode {
        ExportMode::Student => OutputProfile::Student,
        ExportMode::Teacher => OutputProfile::Teacher,
        ExportMode::Exam => OutputProfile::Exam,
    }
}
/// 定稿版面参数
///
/// 请求带 `spec` 就**整体替换**预设 —— 前端的 PDF 展开区是「先选预设再微调」，发出来的必然
/// 是一份完整的 spec（T3.2 的 `#[serde(default)]` 链保证缺字段也补得齐），因此不需要计划正文
/// 里说的字段级合并（那要求区分「没填」与「填了默认值」，得为 LayoutSpec 再造一套全 Option 的
/// patch 类型并同步导出 TS，M3 不划算）。`profile` 一律回填 mode 的翻译结果：**模式是权威**。
pub fn resolve_spec(profile: OutputProfile, request_spec: Option<&LayoutSpec>) -> LayoutSpec {
    let mut spec = request_spec
        .cloned()
        .unwrap_or_else(|| LayoutSpec::for_profile(profile));
    spec.profile = profile;
    spec
}

/// 本张卷子的选项可用栏宽（em）
fn available_em(spec: &LayoutSpec) -> f64 {
    let mm = f64::from(spec.column_width_mm());
    (choice_grid::em_from_mm(mm) - HANGING_EM).max(1.0)
}

fn document_meta(bundle: &ExamBundle) -> DocumentMeta {
    let total_score = bundle.exam_meta.total_score.unwrap_or_else(|| {
        bundle
            .sections
            .iter()
            .flat_map(|s| s.questions.iter())
            .map(|q| q.score)
            .sum()
    });
    DocumentMeta {
        school: bundle.exam_meta.school.clone(),
        duration_min: bundle.exam_meta.duration,
        total_score,
        instructions: bundle.exam_meta.instructions.clone(),
    }
}

// ═══════════════════════════ 大题与题块 ═══════════════════════════

fn section(sec: &ExamSection, ctx: &BlockCtx) -> Section {
    Section {
        header: SectionHeader {
            // 大题标题落在页尾就是废行，永远粘住下一块
            meta: BlockMeta::attach(),
            title: sec.title.clone(),
            instruction: sec.instruction.clone(),
            question_count: sec.questions.len(),
            total_score: sec.questions.iter().map(|q| q.score).sum(),
        },
        blocks: sec
            .questions
            .iter()
            .flat_map(|q| question_blocks(q, ctx))
            .collect(),
    }
}

/// 单题 → 线性块序列：题面（题干 →（小问）→（留白））由题型模板出，
/// 其后才是模式差异（Callout → 内嵌答案）
///
/// 收尾清掉本题最后一块的 `keep_with_next`：粘连只在一题之内。模板给题干块（乃至最后一个小问）
/// 置了这个位，那是「题干粘住它自己的小问/留白」的意思，一路传到序列末尾就会把两道题焊成一块。
fn question_blocks(q: &ExamQuestion, ctx: &BlockCtx) -> Vec<LayoutBlock> {
    let mut out = ctx.registry.expand(q, ctx, &split_content);
    for callout in &q.callouts {
        out.push(LayoutBlock::Callout(CalloutBlock {
            meta: BlockMeta::flow(),
            number: q.number,
            callout: callout.clone(),
        }));
    }
    // 答案内嵌（`answer_at_end = false`）时排在题末；卷末答案由 `LayoutDoc::answer_key` 承载
    if !ctx.options.answer_at_end {
        out.extend(answer_block(q, ctx, BlockMeta::flow()).map(LayoutBlock::Answer));
    }
    if let Some(last) = out.last_mut() {
        last.meta_mut().keep_with_next = false;
    }
    out
}

/// 卷级样式冲突只报一条（逐题报会把 `X-Export-Warnings` 头撑爆，B3）
fn blank_conflicts(options: &ExportOptions, spec: &LayoutSpec) -> Vec<Issue> {
    let Some(space) = options.answer_space else {
        return Vec::new();
    };
    if space.style == to_wire_style(spec.answer_blank.style) {
        return Vec::new();
    }
    vec![Issue {
        question_no: None,
        field: IssueField::Other,
        severity: IssueSeverity::Info,
        latex: None,
        reason: format!(
            "留白样式以导出选项为准：{}（版面预设为 {}）",
            style_label(space.style),
            style_name(spec.answer_blank.style)
        ),
    }]
}

fn to_wire_style(s: BlankStyle) -> WireBlankStyle {
    match s {
        BlankStyle::Lines => WireBlankStyle::Lines,
        BlankStyle::Dots => WireBlankStyle::Dots,
        BlankStyle::Blank => WireBlankStyle::Blank,
    }
}

fn style_label(s: WireBlankStyle) -> &'static str {
    match s {
        WireBlankStyle::Lines => "横线格",
        WireBlankStyle::Dots => "点阵",
        WireBlankStyle::Blank => "纯空白",
    }
}

fn style_name(s: BlankStyle) -> &'static str {
    match s {
        BlankStyle::Lines => "横线格",
        BlankStyle::Dots => "点阵",
        BlankStyle::Blank => "纯空白",
    }
}

// ═══════════════════════════ 答案与解析 ═══════════════════════════

fn answer_block(q: &ExamQuestion, ctx: &BlockCtx, meta: BlockMeta) -> Option<AnswerBlock> {
    let lines = answer_lines(q, ctx);
    let analyses = analyses(q, ctx.options);
    if lines.is_empty() && analyses.is_empty() {
        return None;
    }
    Some(AnswerBlock {
        meta,
        number: q.number,
        kind: q.kind,
        lines,
        analyses,
    })
}

/// 答案行：解答题逐小问（带小问号），其余题型一条（多空用「；」串起来）
///
/// 「逐小问」的判据与出块侧同源（`policy.expands_parts`）—— 排了小问却不逐小问给答案，
/// 或反过来，都是同一处漏改。
fn answer_lines(q: &ExamQuestion, ctx: &BlockCtx) -> Vec<AnswerLine> {
    if !ctx.options.include_answer {
        return Vec::new();
    }
    let written = ctx.registry.builder(q.kind).policy().expands_parts;
    if written && !q.structure_parts.is_empty() {
        return walk_leaves(&q.structure_parts)
            .into_iter()
            .filter_map(|p| {
                let text = p.answer.as_deref().unwrap_or("").trim();
                if text.is_empty() {
                    return None;
                }
                Some(AnswerLine {
                    label: p.label.clone(),
                    nodes: split_content(text),
                })
            })
            .collect();
    }
    let joined = join_answers(&q.answers);
    if joined.is_empty() {
        return Vec::new();
    }
    vec![AnswerLine {
        label: String::new(),
        nodes: joined,
    }]
}

/// 逐条切分后再用「；」相连 —— 先拼字符串再切分会让 `$…$` 跨条配对
fn join_answers(answers: &[String]) -> Vec<InlineNode> {
    let mut out: Vec<InlineNode> = Vec::new();
    for a in answers {
        let text = a.trim();
        if text.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(InlineNode::Text {
                text: "；".to_string(),
            });
        }
        out.extend(split_content(text));
    }
    out
}

fn analyses(q: &ExamQuestion, options: &ExportOptions) -> Vec<AnalysisEntry> {
    if !options.include_analysis {
        return Vec::new();
    }
    q.analyses
        .iter()
        .map(|b| AnalysisEntry {
            title: b.title.clone(),
            nodes: split_content(b.content.trim()),
        })
        .filter(|e| !e.nodes.is_empty())
        .collect()
}

// ═══════════════════════════ PDF 出口（T3.7） ═══════════════════════════

/// typst 认得的图片格式。`infer` 能嗅出来的类型远多于这一列（tif/bmp/ico/heic…），
/// 闸门必须自己把关：一张解不了的图会让**整次编译**开天窗，比 docx 少嵌一张图严重。
const RENDERABLE: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "svg"];

/// PDF 生成结果
pub struct PdfResult {
    pub bytes: Vec<u8>,
    /// 生成期新问题（素材、公式降级、字体回退）；与题级 issues 合并进 X-Export-Warnings
    pub issues: Vec<Issue>,
}

/// LayoutDoc → main.typ → PDF：排版出口的唯一渲染路径（R1，不提供 `/typeset/render`）
///
/// 编译失败是唯一能让整卷开天窗的失败：坏公式在 [`typst_gen::generate`] 里逐枚降级、坏图在
/// [`prefetch_assets`] 里摘掉，还在出错就是模板或环境的问题。把诊断原样交给 handler（500），
/// 不在这里假装修好 —— 静默产出一份缺页的考卷比报错更糟。
pub async fn generate_pdf(doc: &LayoutDoc, upload_dir: &Path) -> Result<PdfResult, CompileError> {
    let assets = prefetch_assets(doc, upload_dir).await;
    let generated = typst_gen::generate(doc, &assets.images);

    let dirs = font_dirs();
    let compiled = compile_pdf(&CompileRequest {
        source: &generated.source,
        upload_dir,
        font_dirs: &dirs,
        injected: &assets.injected,
    })?;

    let mut issues = assets.issues;
    issues.extend(generated.issues);
    // §13.4：缺中文字体照常排，但豆腐块必须可诊断 —— 能回退不是静默的理由
    let missing = missing_cjk_fonts(&dirs);
    if !missing.is_empty() {
        issues.push(Issue {
            question_no: None,
            field: IssueField::Other,
            severity: IssueSeverity::Warning,
            latex: None,
            reason: format!(
                "缺少中文字体：{}，中文按字体回退渲染（可能出现豆腐块）",
                missing.join("、")
            ),
        });
    }
    // typst 自己的告警（字形缺失、弃用写法等）：卷级，源码是我们生成的，行号对教师无意义
    issues.extend(compiled.warnings.into_iter().map(|warning| Issue {
        question_no: None,
        field: IssueField::Other,
        severity: IssueSeverity::Warning,
        latex: None,
        reason: format!("typst：{warning}"),
    }));

    Ok(PdfResult {
        bytes: compiled.output,
        issues,
    })
}

/// 渲染素材：`typst_gen` 要的 URL 表 + `compiler` 要的注入字节 + 预取期发现的问题
#[derive(Default)]
struct Assets {
    /// 值 `None` = 这张图拿不到且**已记 Issue**（见 [`typst_gen::generate`] 的口径）
    images: HashMap<String, Option<String>>,
    injected: Vec<(String, Vec<u8>)>,
    issues: Vec<Issue>,
}

/// 预取版面上的全部图片（B1）
///
/// 抓一遍才准：typst 的 `#image()` 打不开或解不出文件是整次编译失败，一个丢了的文件能把
/// 整卷变成 500。本地图与外链走同一个入口 `assets::fetch_image`，区别只在外链要过网络。
async fn prefetch_assets(doc: &LayoutDoc, upload_dir: &Path) -> Assets {
    let mut a = Assets::default();
    for (qno, url) in collect_images(doc) {
        if a.images.contains_key(&url) {
            continue; // 同图复用：一次抓取、一份字节、一个注入名
        }
        let path = match fetch_image(&url, upload_dir).await {
            Ok(img) => a.register(qno, &url, img),
            Err(e) => {
                a.issues.push(Issue {
                    question_no: qno,
                    field: IssueField::Image,
                    severity: IssueSeverity::Warning,
                    latex: None,
                    reason: format!("图片 {url} 处理失败：{e}"),
                });
                None
            }
        };
        a.images.insert(url, path);
    }
    a
}

impl Assets {
    /// 登记一张抓到的图。
    ///
    /// 一律注入 `/ext/<n>.<ext>` 而不是让 typst 直接读盘：typst 按**路径扩展名**选解码器，
    /// 而库里的 URL 后缀与真实格式对不对得上没人保证（`.jpg` 里装着 PNG 是上传侧常事）。
    /// 序号命名而非 URL 哈希见 [`crate::typeset::compiler`] 模块头的 FileId interner 说明。
    fn register(&mut self, qno: Option<u32>, url: &str, img: FetchedImage) -> Option<String> {
        let ext = img.ext.to_ascii_lowercase();
        if !RENDERABLE.contains(&ext.as_str()) {
            self.issues.push(Issue {
                question_no: qno,
                field: IssueField::Image,
                severity: IssueSeverity::Warning,
                latex: None,
                reason: format!("图片 {url} 是 {ext}，PDF 不嵌入（仅支持 PNG/JPEG/GIF/WebP/SVG）"),
            });
            return None;
        }
        let path = format!("/ext/{}.{}", self.injected.len(), ext);
        self.injected.push((path.clone(), img.bytes));
        Some(path)
    }
}

/// 版面上会出现的图片 URL（带归属题号）。IR 已是切分后的 `InlineNode`，无需二次解析文本。
fn collect_images(doc: &LayoutDoc) -> Vec<(Option<u32>, String)> {
    let mut out = Vec::new();
    for section in &doc.sections {
        for block in &section.blocks {
            block_images(block, &mut out);
        }
    }
    for answer in &doc.answer_key {
        answer_images(answer, &mut out);
    }
    out
}

fn block_images(block: &LayoutBlock, out: &mut Vec<(Option<u32>, String)>) {
    let qno = Some(block.question_no());
    match block {
        LayoutBlock::Question(q) => {
            push_nodes(&q.stem, qno, out);
            for option in &q.options {
                push_nodes(&option.content, qno, out);
            }
        }
        LayoutBlock::SubQuestion(s) => push_nodes(&s.stem, qno, out),
        LayoutBlock::Callout(c) => push_nodes(&c.callout.nodes, qno, out),
        LayoutBlock::Answer(a) => answer_images(a, out),
        LayoutBlock::Blank(_) => {}
    }
}

fn answer_images(answer: &AnswerBlock, out: &mut Vec<(Option<u32>, String)>) {
    let qno = Some(answer.number);
    for line in &answer.lines {
        push_nodes(&line.nodes, qno, out);
    }
    for entry in &answer.analyses {
        push_nodes(&entry.nodes, qno, out);
    }
}

fn push_nodes(nodes: &[InlineNode], qno: Option<u32>, out: &mut Vec<(Option<u32>, String)>) {
    for node in nodes {
        match node {
            InlineNode::Image { url, .. } => out.push((qno, url.clone())),
            InlineNode::ImgRow { images, .. } => {
                out.extend(images.iter().map(|i| (qno, i.url.clone())));
            }
            _ => {}
        }
    }
}

// ═══════════════════════════ 测试 ═══════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::model::{
        AnswerSpace, Callout, CalloutKind, ExamMeta, ExamOption, ExamSection, QuestionKind,
    };
    use crate::models::question_structure::{AnalysisBlock, QuestionPart};
    use crate::typeset::blocks::{BlockBuilder, Policy, Registry};
    use crate::typeset::ir::{BlankBlock, QuestionBlock};
    use crate::typeset::spec::{BlankSpec, Margins};

    fn text(s: &str) -> InlineNode {
        InlineNode::Text { text: s.into() }
    }

    fn nodes(s: &str) -> Vec<InlineNode> {
        split_content(s)
    }

    /// 把节点序列压回纯文本（断言答案串与切分结果最省事）
    fn plain(ns: &[InlineNode]) -> String {
        ns.iter()
            .map(|n| match n {
                InlineNode::Text { text } => text.clone(),
                InlineNode::LineBreak => "\n".to_string(),
                InlineNode::Math { latex, display } => {
                    if *display {
                        format!("$${latex}$$")
                    } else {
                        format!("${latex}$")
                    }
                }
                InlineNode::Image { url, .. } => format!("[{url}]"),
                InlineNode::ImgRow { images, .. } => {
                    images.iter().map(|i| format!("[{}]", i.url)).collect()
                }
                InlineNode::Table { header, .. } => header.join("|"),
            })
            .collect()
    }

    fn space(style: WireBlankStyle, height_cm: f64) -> AnswerSpace {
        AnswerSpace { style, height_cm }
    }

    /// 四个短选项的单选题（`letters` 是答案，长度 >1 即多选）
    fn choice(number: u32, letters: &[&str]) -> ExamQuestion {
        ExamQuestion {
            number,
            score: 5.0,
            kind: if letters.len() > 1 {
                QuestionKind::MultiChoice
            } else {
                QuestionKind::SingleChoice
            },
            stem: nodes("设集合 $A=\\{1\\}$，则（　）"),
            options: ["A", "B", "C", "D"]
                .iter()
                .map(|l| ExamOption {
                    label: (*l).to_string(),
                    content: vec![text("1")],
                })
                .collect(),
            answers: letters.iter().map(|s| s.to_string()).collect(),
            analyses: vec![],
            structure_parts: vec![],
            callouts: vec![],
            answer_space: None,
            issues: vec![],
        }
    }

    /// 选项宽约 9.4em（标签 1.4 + 8 个 CJK）：单栏够排 4 列，三栏只够 2 列
    fn medium_choice(number: u32) -> ExamQuestion {
        let mut q = choice(number, &["A"]);
        for o in &mut q.options {
            o.content = vec![text("甲乙丙丁戊己庚辛")];
        }
        q
    }

    /// 无选项的写作题（解答/综合题骨架）
    fn written(number: u32, kind: QuestionKind) -> ExamQuestion {
        ExamQuestion {
            number,
            score: 10.0,
            kind,
            stem: nodes("求下列函数的值域。"),
            options: vec![],
            answers: vec![],
            analyses: vec![],
            structure_parts: vec![],
            callouts: vec![],
            answer_space: None,
            issues: vec![],
        }
    }

    fn part(id: &str, label: &str, stem: &str, answer: &str) -> QuestionPart {
        QuestionPart {
            id: id.into(),
            label: label.into(),
            stem: stem.into(),
            children: vec![],
            answer: (!answer.is_empty()).then(|| answer.into()),
            analyses: vec![],
            no_analysis_needed: false,
            label_dirty: false,
        }
    }

    fn bundle(mode: ExportMode, questions: Vec<ExamQuestion>) -> ExamBundle {
        ExamBundle {
            title: "2026 期中测试".into(),
            subtitle: Some("数学试卷".into()),
            exam_meta: ExamMeta {
                school: Some("实验中学".into()),
                duration: Some(120),
                total_score: None,
                instructions: vec!["请将答案写在答题区".into()],
            },
            mode,
            sections: vec![ExamSection {
                title: "一、单选题".into(),
                instruction: Some("每题 5 分".into()),
                questions,
            }],
        }
    }

    fn one(mode: ExportMode, q: ExamQuestion, options: &ExportOptions) -> LayoutDoc {
        build_layout_doc(&bundle(mode, vec![q]), options, None)
    }

    /// 块类型标签序列（断言顺序最省事）
    fn shape(blocks: &[LayoutBlock]) -> Vec<&'static str> {
        blocks
            .iter()
            .map(|b| match b {
                LayoutBlock::Question(_) => "question",
                LayoutBlock::SubQuestion(_) => "sub",
                LayoutBlock::Callout(_) => "callout",
                LayoutBlock::Blank(_) => "blank",
                LayoutBlock::Answer(_) => "answer",
            })
            .collect()
    }

    fn question_of(doc: &LayoutDoc) -> &QuestionBlock {
        doc.sections[0]
            .blocks
            .iter()
            .find_map(|b| match b {
                LayoutBlock::Question(q) => Some(q),
                _ => None,
            })
            .expect("question block")
    }

    fn blank_of(doc: &LayoutDoc) -> Option<&BlankBlock> {
        doc.sections[0].blocks.iter().find_map(|b| match b {
            LayoutBlock::Blank(x) => Some(x),
            _ => None,
        })
    }

    // ── 题型映射 ──

    #[test]
    fn single_choice_becomes_one_glued_question_block() {
        let doc = one(
            ExportMode::Student,
            choice(1, &["A"]),
            &ExportOptions::default(),
        );
        assert_eq!(shape(&doc.sections[0].blocks), vec!["question"]);
        let q = question_of(&doc);
        assert_eq!(q.kind, QuestionKind::SingleChoice);
        assert_eq!(q.options.len(), 4);
        assert_eq!(
            (q.grid.columns, q.grid.rows),
            (4, 1),
            "短选项 A4 双栏排 4 列"
        );
        // 短题整块不跨页；它是本题最后一块，链已收尾（不许把下一题焊进来）
        assert!(!q.meta.breakable);
        assert!(!q.meta.keep_with_next);
    }

    #[test]
    fn keep_chain_is_closed_inside_its_own_question() {
        let mut q1 = written(1, QuestionKind::Solution);
        q1.structure_parts = vec![part("(1)", "第一问", "求 f 的解析式。", "f(x) = x^2。")];
        let q2 = written(2, QuestionKind::Solution);
        let options = ExportOptions {
            answer_space: Some(space(WireBlankStyle::Lines, 6.0)),
            ..ExportOptions::default()
        };
        let doc = build_layout_doc(&bundle(ExportMode::Student, vec![q1, q2]), &options, None);
        let blocks = &doc.sections[0].blocks;
        assert_eq!(
            shape(blocks),
            ["question", "sub", "blank", "question", "blank"]
        );
        assert_eq!(
            blocks
                .iter()
                .map(|b| b.meta().keep_with_next)
                .collect::<Vec<_>>(),
            // 题面粘住小问、小问粘住自己的作答区，但链不许跨过题界
            vec![true, true, false, true, false],
            "粘连只在一题之内"
        );
    }

    #[test]
    fn tall_content_forbids_gluing_the_whole_question() {
        let mut q = choice(1, &["A"]);
        q.stem = nodes("如图：\n\n| x | y |\n|---|---|\n| 1 | 2 |\n\n求面积。");
        let doc = one(ExportMode::Student, q, &ExportOptions::default());
        assert!(
            question_of(&doc).meta.breakable,
            "含表格的题干可能超页，必须允许断开"
        );
    }

    #[test]
    fn fill_keeps_the_dug_stem_and_never_gets_a_slab_of_blank() {
        let q = ExamQuestion {
            stem: nodes("已知 $f(x)=x^2$，则 $f(2)=$ ______。"),
            ..written(3, QuestionKind::Fill)
        };
        // 即使卷面开了留白，填空题也不给整块作答区（作答位在行内下划线上，B2）
        let options = ExportOptions {
            answer_space: Some(space(WireBlankStyle::Lines, 6.0)),
            ..ExportOptions::default()
        };
        let doc = one(ExportMode::Student, q, &options);
        assert_eq!(shape(&doc.sections[0].blocks), vec!["question"]);
        let math: Vec<String> = doc.sections[0]
            .blocks
            .iter()
            .flat_map(|b| match b {
                LayoutBlock::Question(q) => q.stem.clone(),
                _ => vec![],
            })
            .filter_map(|n| match n {
                InlineNode::Math { latex, .. } => Some(latex),
                _ => None,
            })
            .collect();
        assert_eq!(math, vec!["f(x)=x^2", "f(2)="], "行内公式原样进 IR");
    }

    #[test]
    fn solution_expands_the_tree_then_reserves_answer_space() {
        let q = ExamQuestion {
            stem: nodes("已知函数 $f(x)=x^3-3x$。"),
            structure_parts: vec![
                part(
                    "p1",
                    "(1)",
                    "求 $f(x)$ 的单调区间。",
                    "在 $(1,+\\infty)$ 递增",
                ),
                {
                    let mut branch = part("p2", "(2)", "继续求解。", "");
                    branch.children = vec![part("p2a", "①", "求极小值。", "-2")];
                    branch
                },
            ],
            ..written(5, QuestionKind::Solution)
        };
        let options = ExportOptions {
            answer_space: Some(space(WireBlankStyle::Lines, 6.0)),
            ..ExportOptions::default()
        };
        let doc = one(ExportMode::Student, q, &options);
        assert_eq!(
            shape(&doc.sections[0].blocks),
            vec!["question", "sub", "sub", "sub", "blank"]
        );
        let tree: Vec<(u8, String)> = doc.sections[0]
            .blocks
            .iter()
            .filter_map(|b| match b {
                LayoutBlock::SubQuestion(s) => Some((s.depth, s.label.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            tree,
            vec![(0, "(1)".into()), (0, "(2)".into()), (1, "①".into())],
            "分支节点与其子节点分层缩进"
        );
        assert!(question_of(&doc).meta.breakable, "解答题不整块锁死");
        let blank = blank_of(&doc).expect("解答题留白");
        assert_eq!(blank.style, BlankStyle::Lines);
        assert!((blank.height_mm - 60.0).abs() < 1e-4);
    }

    #[test]
    fn composite_is_laid_out_like_a_solution() {
        // §7.4 第 5 桶：DB 枚举里没有 composite，适配器仍要在纸上给它位置
        let options = ExportOptions {
            answer_space: Some(space(WireBlankStyle::Dots, 4.0)),
            ..ExportOptions::default()
        };
        let doc = one(
            ExportMode::Exam,
            written(9, QuestionKind::Composite),
            &options,
        );
        assert_eq!(shape(&doc.sections[0].blocks), vec!["question", "blank"]);
        let blank = blank_of(&doc).expect("综合题留白");
        assert_eq!(blank.style, BlankStyle::Dots);
        assert!((blank.height_mm - 40.0).abs() < 1e-4);
    }

    // ── 三模式 ──

    #[test]
    fn three_modes_pick_three_presets() {
        for (mode, profile, columns, paper) in [
            (
                ExportMode::Student,
                OutputProfile::Student,
                2_u8,
                (210, 297),
            ),
            (ExportMode::Teacher, OutputProfile::Teacher, 1, (210, 297)),
            (ExportMode::Exam, OutputProfile::Exam, 2, (420, 297)),
        ] {
            let doc = one(mode, choice(1, &["A"]), &ExportOptions::default());
            assert_eq!(doc.profile, profile, "{mode:?} 的口径");
            assert_eq!(doc.spec.profile, profile, "spec 里的 profile 跟随模式");
            assert_eq!(doc.spec.columns, columns, "{mode:?} 的默认栏数");
            assert_eq!(doc.spec.paper.size_mm(), paper, "{mode:?} 的纸面");
        }
    }

    #[test]
    fn request_spec_replaces_the_preset_but_not_the_mode() {
        let patch = LayoutSpec {
            columns: 1,
            ..LayoutSpec::preset("a3_tri_exam").expect("preset")
        };
        let doc = build_layout_doc(
            &bundle(ExportMode::Exam, vec![choice(1, &["A"])]),
            &ExportOptions::default(),
            Some(&patch),
        );
        assert_eq!(doc.spec.columns, 1, "请求里的 spec 整体生效");
        assert_eq!(doc.spec.paper.size_mm(), (420, 297));
        assert_eq!(
            doc.spec.profile,
            OutputProfile::Exam,
            "模式是权威：spec.profile 由 mode 回填"
        );
    }

    #[test]
    fn grid_columns_follow_the_column_width() {
        // 同一份选项（≈9.4em）：A4 单栏（45em）排 4 列，A3 三栏（31.5em）只够 2 列
        let q = medium_choice(1);
        let wide = build_layout_doc(
            &bundle(ExportMode::Teacher, vec![q.clone()]),
            &ExportOptions::default(),
            Some(&LayoutSpec::preset("a4_lecture").expect("preset")),
        );
        let narrow = build_layout_doc(
            &bundle(ExportMode::Exam, vec![q]),
            &ExportOptions::default(),
            Some(&LayoutSpec::preset("a3_tri_exam").expect("preset")),
        );
        assert_eq!(question_of(&wide).grid.columns, 4);
        assert_eq!(question_of(&narrow).grid.columns, 2);
    }

    // ── Callout ──

    #[test]
    fn teacher_callouts_follow_their_question() {
        let q = ExamQuestion {
            callouts: vec![
                Callout {
                    kind: CalloutKind::Knowledge,
                    title: "考点清单".into(),
                    nodes: vec![text("函数的单调性")],
                },
                Callout {
                    kind: CalloutKind::ErrorProne,
                    title: "易错警示".into(),
                    nodes: vec![text("漏讨论端点")],
                },
            ],
            ..choice(2, &["A"])
        };
        let doc = one(ExportMode::Teacher, q, &ExportOptions::default());
        assert_eq!(
            shape(&doc.sections[0].blocks),
            vec!["question", "callout", "callout"],
            "Callout 紧跟所属题块"
        );
        assert_eq!(doc.answer_key.len(), 1, "默认卷末答案，不内嵌");
        let kinds: Vec<CalloutKind> = doc.sections[0]
            .blocks
            .iter()
            .filter_map(|b| match b {
                LayoutBlock::Callout(c) => Some(c.callout.kind),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, vec![CalloutKind::Knowledge, CalloutKind::ErrorProne]);
        assert_eq!(doc.sections[0].blocks[1].question_no(), 2, "警告定位靠题号");
    }

    // ── 留白合并（B5） ──

    fn blank_case(mode: ExportMode, options: &ExportOptions) -> LayoutDoc {
        one(mode, written(1, QuestionKind::Solution), options)
    }

    #[test]
    fn options_decide_switch_and_height_spec_decides_style() {
        // 开关：options 没给 answer_space → 整卷不留白，spec 的兜底高度不许自己生效
        let doc = blank_case(ExportMode::Student, &ExportOptions::default());
        assert!(blank_of(&doc).is_none());
        assert!(doc.issues.is_empty());

        // 高度：options 说了算（4cm → 40mm）
        let options = ExportOptions {
            answer_space: Some(space(WireBlankStyle::Lines, 4.0)),
            ..ExportOptions::default()
        };
        let doc = blank_case(ExportMode::Student, &options);
        assert!((blank_of(&doc).unwrap().height_mm - 40.0).abs() < 1e-4);

        // 高度没填（0）→ 退回 spec 的 6cm
        let options = ExportOptions {
            answer_space: Some(space(WireBlankStyle::Lines, 0.0)),
            ..ExportOptions::default()
        };
        let doc = blank_case(ExportMode::Student, &options);
        assert!((blank_of(&doc).unwrap().height_mm - 60.0).abs() < 1e-4);

        // 样式：spec 决定，且 options 与之相符时不必多嘴
        let dotted = LayoutSpec {
            answer_blank: BlankSpec {
                style: BlankStyle::Dots,
                ..BlankSpec::default()
            },
            ..LayoutSpec::default()
        };
        let options = ExportOptions {
            answer_space: Some(space(WireBlankStyle::Dots, 2.0)),
            ..ExportOptions::default()
        };
        let doc = build_layout_doc(
            &bundle(
                ExportMode::Student,
                vec![written(1, QuestionKind::Solution)],
            ),
            &options,
            Some(&dotted),
        );
        assert_eq!(blank_of(&doc).unwrap().style, BlankStyle::Dots);
        assert!(doc.issues.is_empty());

        // 冲突：以 options 为准，并记一条卷级 info
        let options = ExportOptions {
            answer_space: Some(space(WireBlankStyle::Lines, 2.0)),
            ..ExportOptions::default()
        };
        let doc = build_layout_doc(
            &bundle(
                ExportMode::Student,
                vec![written(1, QuestionKind::Solution)],
            ),
            &options,
            Some(&dotted),
        );
        assert_eq!(blank_of(&doc).unwrap().style, BlankStyle::Lines);
        assert_eq!(doc.issues.len(), 1);
        assert_eq!(doc.issues[0].severity, IssueSeverity::Info);
        assert_eq!(doc.issues[0].question_no, None, "卷级冲突只报一条");
        assert!(
            doc.issues[0].reason.contains("点阵"),
            "{}",
            doc.issues[0].reason
        );
    }

    #[test]
    fn question_level_space_overrides_the_request_level() {
        let q = ExamQuestion {
            answer_space: Some(space(WireBlankStyle::Dots, 2.0)),
            ..written(1, QuestionKind::Solution)
        };
        let doc = one(ExportMode::Student, q, &ExportOptions::default());
        let blank = blank_of(&doc).expect("题级覆盖本身就是开关");
        assert_eq!(blank.style, BlankStyle::Dots);
        assert!((blank.height_mm - 20.0).abs() < 1e-4);
        assert!(doc.issues.is_empty(), "题级覆盖是显式意图，不算冲突");
    }

    #[test]
    fn teacher_handout_folds_blanks_into_analysis() {
        // §6.2：留白是学生/考卷口径的事，讲义版折叠为解析
        let options = ExportOptions {
            answer_space: Some(space(WireBlankStyle::Lines, 6.0)),
            ..ExportOptions::default()
        };
        let doc = blank_case(ExportMode::Teacher, &options);
        assert!(blank_of(&doc).is_none(), "讲义不给作答区");
    }

    // ── 答案位置与开关 ──

    fn tree_question() -> ExamQuestion {
        ExamQuestion {
            structure_parts: vec![
                part("a", "(1)", "求 $a_1$。", "$2$"),
                part("b", "(2)", "求公比。", r"$\frac{1}{2}$"),
            ],
            ..written(4, QuestionKind::Solution)
        }
    }

    #[test]
    fn answers_go_to_the_end_of_the_paper_when_asked() {
        let options = ExportOptions {
            answer_at_end: true,
            ..ExportOptions::default()
        };
        let doc = one(ExportMode::Exam, tree_question(), &options);
        assert_eq!(
            shape(&doc.sections[0].blocks),
            vec!["question", "sub", "sub"]
        );
        assert!(doc.has_answer_key());
        assert_eq!(doc.answer_key.len(), 1);
        let key = &doc.answer_key[0];
        assert_eq!(key.number, 4);
        assert_eq!(key.lines.len(), 2, "解答题逐小问出行");
        assert_eq!(key.lines[0].label, "(1)");
        assert_eq!(plain(&key.lines[0].nodes), "$2$");
        assert_eq!(plain(&key.lines[1].nodes), r"$\frac{1}{2}$");
    }

    #[test]
    fn answers_fold_into_the_question_when_inline() {
        let options = ExportOptions {
            answer_at_end: false,
            ..ExportOptions::default()
        };
        let doc = one(ExportMode::Student, choice(1, &["B", "D"]), &options);
        assert!(doc.answer_key.is_empty());
        assert_eq!(shape(&doc.sections[0].blocks), vec!["question", "answer"]);
        let LayoutBlock::Answer(a) = &doc.sections[0].blocks[1] else {
            panic!("期望答案块");
        };
        assert_eq!(a.lines.len(), 1);
        assert_eq!(
            plain(&a.lines[0].nodes),
            "B；D",
            "逐条切分后再用「；」串起来"
        );
    }

    #[test]
    fn analysis_and_answer_are_independent_switches() {
        let mut q = choice(1, &["A"]);
        q.answers = vec![];
        q.analyses = vec![AnalysisBlock {
            id: "analysis".into(),
            title: String::new(),
            content: "先化简再代入。".into(),
        }];
        let options = ExportOptions {
            include_answer: false,
            include_analysis: true,
            answer_at_end: true,
            ..ExportOptions::default()
        };
        // 答案关掉、解析开着：卷末区仍要出块
        let doc = one(ExportMode::Teacher, q, &options);
        assert_eq!(doc.answer_key.len(), 1);
        assert!(doc.answer_key[0].lines.is_empty());
        assert_eq!(
            plain(&doc.answer_key[0].analyses[0].nodes),
            "先化简再代入。"
        );

        // 两个开关都关 → 一块不出
        let doc = one(
            ExportMode::Teacher,
            choice(1, &["A"]),
            &ExportOptions {
                include_answer: false,
                ..ExportOptions::default()
            },
        );
        assert!(doc.answer_key.is_empty());
        assert!(!doc.has_answer_key());
    }

    // ── 稳健性 ──

    #[test]
    fn empty_and_degenerate_input_produce_no_phantom_blocks() {
        let empty = ExamBundle {
            title: "空卷".into(),
            subtitle: None,
            exam_meta: ExamMeta::default(),
            mode: ExportMode::Student,
            sections: vec![
                ExamSection {
                    title: "一、".into(),
                    instruction: None,
                    questions: vec![],
                },
                ExamSection {
                    title: "二、".into(),
                    instruction: None,
                    questions: vec![ExamQuestion {
                        stem: vec![],
                        structure_parts: vec![QuestionPart {
                            children: vec![part("c", "(1)", "有内容的小问。", "")],
                            ..part("b", "", "", "")
                        }],
                        ..written(1, QuestionKind::Solution)
                    }],
                },
            ],
        };
        let doc = build_layout_doc(&empty, &ExportOptions::default(), None);
        assert_eq!(doc.question_count(), 1, "空大题不计题");
        assert!(doc.sections[0].blocks.is_empty());
        assert_eq!(shape(&doc.sections[1].blocks), vec!["question", "sub"]);
        assert_eq!(doc.meta.total_score, 10.0, "总分按各题求和兜底");

        // 零宽栏（荒谬边距）也不许把栅格算崩
        let cramped = LayoutSpec {
            columns: 3,
            margins: Margins {
                left_mm: 200.0,
                right_mm: 200.0,
                ..Margins::default()
            },
            ..LayoutSpec::default()
        };
        let doc = build_layout_doc(
            &bundle(ExportMode::Student, vec![choice(1, &["A"])]),
            &ExportOptions::default(),
            Some(&cramped),
        );
        let grid = question_of(&doc).grid;
        assert!((1..=4).contains(&grid.columns), "{grid:?}");
    }

    // ── 素材预取与 PDF 出口（T3.7） ──

    /// 1×1 灰块 SVG：typst 真解得动，又能当文本常量写进断言
    const DOT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#333"/></svg>"##;
    /// 最小 PNG 签名：`infer` 认得，够预取层用（typst 解码才需要真图）
    const PNG_SIG: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    fn uploads_with(files: &[(&str, &[u8])]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("mathset-pdf-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, bytes) in files {
            let path = dir.join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
        }
        dir
    }

    /// 单题卷子，题干按 markdown 图片语法写（`![alt](url)` 必须独占一行）
    fn image_doc(stem: &str) -> LayoutDoc {
        let q = ExamQuestion {
            stem: nodes(stem),
            ..choice(1, &["A"])
        };
        build_layout_doc(
            &bundle(ExportMode::Student, vec![q]),
            &ExportOptions::default(),
            None,
        )
    }

    fn img(ext: &str) -> FetchedImage {
        FetchedImage {
            bytes: vec![9],
            ext: ext.to_string(),
            remote: true,
        }
    }

    #[tokio::test]
    async fn uploads_are_injected_by_ordinal_and_deduped() {
        let dir = uploads_with(&[("questions/a.png", PNG_SIG)]);
        // 同一张图出现两次：只抓一次、只注入一份，两处 #image() 共用一个路径
        let doc = image_doc(
            "如图：\n![甲](/uploads/questions/a.png)\n再看\n![乙](/uploads/questions/a.png)",
        );
        let a = prefetch_assets(&doc, &dir).await;
        assert_eq!(
            a.images.get("/uploads/questions/a.png"),
            Some(&Some("/ext/0.png".to_string())),
            "{:?}",
            a.images
        );
        assert_eq!(a.injected.len(), 1);
        assert_eq!(a.injected[0].0, "/ext/0.png");
        assert_eq!(a.injected[0].1, PNG_SIG);
        assert!(a.issues.is_empty(), "{:?}", a.issues);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[tokio::test]
    async fn unreadable_image_is_skipped_with_the_real_reason() {
        let dir = uploads_with(&[]);
        let doc = image_doc("如图：\n![甲](/uploads/questions/missing.png)");
        let a = prefetch_assets(&doc, &dir).await;
        assert_eq!(
            a.images.get("/uploads/questions/missing.png"),
            Some(&None),
            "拿不到也要进表：值 None 才让下游静默跳图"
        );
        assert!(a.injected.is_empty());
        let issue = &a.issues[0];
        assert_eq!(issue.question_no, Some(1));
        assert_eq!(issue.field, IssueField::Image);
        assert!(issue.reason.contains("不存在"), "{}", issue.reason);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn undecodable_formats_are_gated_out_of_the_package() {
        // tif/bmp/ico 都逃得过 infer 的鼻子，却会让 typst 整次编译失败 —— 必须在这儿拦下
        let mut a = Assets::default();
        assert_eq!(
            a.register(Some(1), "u1", img("png")),
            Some("/ext/0.png".to_string())
        );
        assert_eq!(
            a.register(Some(1), "u2", img("SVG")),
            Some("/ext/1.svg".to_string()),
            "扩展名大小写归一"
        );
        assert_eq!(a.register(Some(2), "u3", img("tif")), None);
        assert_eq!(a.injected.len(), 2, "被拦下的图不进包");
        assert_eq!(a.issues.len(), 1);
        assert_eq!(a.issues[0].question_no, Some(2));
        assert!(a.issues[0].reason.contains("tif"), "{}", a.issues[0].reason);
    }

    #[test]
    fn collect_images_covers_every_block_that_can_carry_one() {
        let mut q = choice(1, &["A"]);
        q.stem = nodes("题干：\n![s](/uploads/s.png)");
        q.options[0].content = nodes("选项：\n![o](/uploads/o.png)");
        q.callouts = vec![Callout {
            kind: CalloutKind::Knowledge,
            title: "考点".into(),
            nodes: nodes("提示：\n![c](/uploads/c.png)"),
        }];
        q.analyses = vec![AnalysisBlock {
            id: "a".into(),
            title: String::new(),
            content: "解析：\n![a](/uploads/a.png)".into(),
        }];
        let doc = build_layout_doc(
            &bundle(ExportMode::Teacher, vec![q]),
            &ExportOptions {
                include_analysis: true,
                ..ExportOptions::default()
            },
            None,
        );
        let found = collect_images(&doc);
        assert_eq!(found.len(), 4, "题干/选项/Callout/解析都要收：{found:?}");
        assert!(
            found.iter().all(|(qno, _)| *qno == Some(1)),
            "警告要能指回题号：{found:?}"
        );
    }

    #[tokio::test]
    async fn generate_pdf_renders_a_real_pdf_with_the_image_embedded() {
        let dir = uploads_with(&[("d.svg", DOT_SVG.as_bytes())]);
        let doc = image_doc("如图：\n![函数图象](/uploads/d.svg)\n求值域。");
        let out = generate_pdf(&doc, &dir)
            .await
            .expect("一张本地图不该让整卷编译失败");
        assert!(out.bytes.starts_with(b"%PDF"), "产物不是 PDF");
        assert!(out.bytes.len() > 1000, "PDF 小得可疑：{}", out.bytes.len());
        let image_issues: Vec<&str> = out
            .issues
            .iter()
            .filter(|i| i.field == IssueField::Image)
            .map(|i| i.reason.as_str())
            .collect();
        assert!(
            image_issues.is_empty(),
            "素材齐了就不该有图片警告：{image_issues:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn one_builder_plus_one_register_reaches_the_rendered_source() {
        // T4.1 的验收口径：新模板 = 一个 builder 实现 + 一行注册。
        // ir / typst_gen / 适配器分派处一行都不改，换的只是查表结果。
        struct FillBlankBuilder;
        impl BlockBuilder for FillBlankBuilder {
            fn kinds(&self) -> &'static [QuestionKind] {
                &[QuestionKind::Fill]
            }
            fn policy(&self) -> Policy {
                Policy {
                    wants_blank: true,
                    compact_stem: true,
                    ..Default::default()
                }
            }
        }
        static CUSTOM: FillBlankBuilder = FillBlankBuilder;

        let options = ExportOptions {
            answer_space: Some(AnswerSpace {
                height_cm: 3.0,
                style: WireBlankStyle::Lines,
            }),
            ..ExportOptions::default()
        };
        let b = bundle(ExportMode::Student, vec![written(1, QuestionKind::Fill)]);

        let default_doc = build_layout_doc(&b, &options, None);
        assert_eq!(shape(&default_doc.sections[0].blocks), ["question"]);

        let registry = Registry::standard().register(&CUSTOM);
        let custom_doc = layout_doc(&b, &options, None, &registry);
        assert_eq!(
            shape(&custom_doc.sections[0].blocks),
            ["question", "blank"],
            "后注册的模板应接管填空题"
        );
        let blank = blank_of(&custom_doc).expect("自定义模板应垫出留白");
        assert_eq!(
            (blank.height_mm, blank.style),
            (30.0, BlankStyle::Lines),
            "留白参数仍按 options × spec 合并"
        );

        // 换表之后的 IR 依旧被渲染层原样吃下
        let rendered = typst_gen::generate(&custom_doc, &HashMap::new());
        assert!(
            rendered.issues.is_empty(),
            "渲染侧不该报问题：{:?}",
            rendered.issues
        );
        assert!(
            rendered.source.contains("#blank-lines("),
            "新块序列没进排版源码"
        );
    }
}
