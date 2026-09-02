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
//!    [`split_content`]，让 IR 只剩 `InlineNode`（见 [`crate::typeset::ir`] 的不变式 1）。

use crate::export::content::split_content;
use crate::export::model::{
    AnswerSpace, BlankStyle as WireBlankStyle, ExamBundle, ExamQuestion, ExamSection, ExportMode,
    ExportOptions, InlineNode, Issue, IssueField, IssueSeverity, QuestionKind,
};
use crate::models::question_structure::{QuestionPart, walk_leaves};
use crate::typeset::blocks::choice_grid::{self, requires_single_column};
use crate::typeset::ir::{
    AnalysisEntry, AnswerBlock, AnswerLine, BlankBlock, BlockMeta, CalloutBlock, DocumentMeta,
    LayoutBlock, LayoutDoc, QuestionBlock, Section, SectionHeader, SubQuestionBlock,
};
use crate::typeset::spec::{BlankStyle, LayoutSpec, OutputProfile, ResolvedBlank};

/// 题号「3.」的悬挂缩进占宽（em）—— docx 侧是 420tw = 2em，两处必须一致
const HANGING_EM: f64 = 2.0;

/// 排版域 IR 的入口：`ExamBundle` + 导出选项（+ 请求里的版面覆盖）→ `LayoutDoc`
pub fn build_layout_doc(
    bundle: &ExamBundle,
    options: &ExportOptions,
    request_spec: Option<&LayoutSpec>,
) -> LayoutDoc {
    let profile = profile_of(bundle.mode);
    let spec = resolve_spec(profile, request_spec);
    let issues = blank_conflicts(options, &spec);
    let ctx = Ctx {
        options,
        spec: &spec,
        profile,
        available_em: available_em(&spec),
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

/// 渲染一份卷子需要知道的上下文（省掉满屏的参数接力）
struct Ctx<'a> {
    options: &'a ExportOptions,
    spec: &'a LayoutSpec,
    profile: OutputProfile,
    /// 选项栅格的可用栏宽（em，已扣掉题号悬挂缩进）
    available_em: f64,
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

fn section(sec: &ExamSection, ctx: &Ctx) -> Section {
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

/// 单题 → 线性块序列：题干 →（小问）→（留白）→（Callout）→（内嵌答案）
fn question_blocks(q: &ExamQuestion, ctx: &Ctx) -> Vec<LayoutBlock> {
    let mut out = Vec::new();

    out.push(LayoutBlock::Question(QuestionBlock {
        meta: question_meta(q),
        number: q.number,
        score: q.score,
        kind: q.kind,
        stem: q.stem.clone(),
        options: q.options.clone(),
        grid: choice_grid::decide(&q.options, ctx.available_em),
    }));

    if is_written(q.kind) {
        parts(q, 0, &mut out);
    }

    if let Some(blank) = blank_block(q, ctx) {
        out.push(LayoutBlock::Blank(blank));
    }
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
    out
}

/// 题干块的分页元数据
///
/// `keep_with_next` 恒真：题干与它下面的选项/小问/留白必须同页起头。
/// `breakable` 只在「短小题」关掉 —— 选择题、填空题在没有留白且不含多行结构（表格、图组、
/// 块级公式、显式换行）时整块高度可控，关掉才拿得到「整题不跨页」；解答题与综合题动辄半页，
/// 强行不跨页只会在 typst 里溢出一页纸。
fn question_meta(q: &ExamQuestion) -> BlockMeta {
    let compact = matches!(
        q.kind,
        QuestionKind::SingleChoice | QuestionKind::MultiChoice | QuestionKind::Fill
    ) && !requires_single_column(&q.stem)
        && !q.options.iter().any(|o| requires_single_column(&o.content));
    if compact {
        BlockMeta::glued()
    } else {
        BlockMeta::attach()
    }
}

/// 问树展开（含分支节点：只存局部题干的那层也要排出来）
fn parts(q: &ExamQuestion, depth: u8, out: &mut Vec<LayoutBlock>) {
    for p in &q.structure_parts {
        push_part(p, depth, q.number, out);
    }
}

fn push_part(p: &QuestionPart, depth: u8, number: u32, out: &mut Vec<LayoutBlock>) {
    let nodes = split_content(&p.stem);
    // 题干为空的分支节点：不占版面，只把子节点抬上来
    if !nodes.is_empty() {
        out.push(LayoutBlock::SubQuestion(SubQuestionBlock {
            meta: BlockMeta::attach(),
            number,
            depth,
            label: p.label.clone(),
            stem: nodes,
        }));
    }
    for c in &p.children {
        push_part(c, depth.saturating_add(1), number, out);
    }
}

// ═══════════════════════════ 留白（B5） ═══════════════════════════

/// 需要作答区的题型：解答题与综合题。
/// 选择题点在选项上，填空题的作答位是题干里的行内下划线（B2 已由装配器挖好），
/// 再垫一块 6cm 横线只会把卷子撑成空白墙。
fn is_written(kind: QuestionKind) -> bool {
    matches!(kind, QuestionKind::Solution | QuestionKind::Composite)
}

/// 本题留白块（`None` = 不留）：教师讲义不留白（答案即解析），其余按 B5 合并
fn blank_block(q: &ExamQuestion, ctx: &Ctx) -> Option<BlankBlock> {
    if !is_written(q.kind) || ctx.profile == OutputProfile::Teacher {
        return None;
    }
    let space = q.answer_space.or(ctx.options.answer_space)?;
    let resolved = resolved_blank(space, ctx.spec)?;
    Some(BlankBlock::new(q.number, &resolved))
}

/// B5 合并：开关与高度在 options，样式在 spec；options 自带样式时以它为准
fn resolved_blank(space: AnswerSpace, spec: &LayoutSpec) -> Option<ResolvedBlank> {
    let mut resolved = spec.resolve_blank(Some(space.height_cm as f32))?;
    resolved.style = style_of(space.style);
    Some(resolved)
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

/// 内容侧样式 → 版面侧样式（两套枚举各自服务各自的 wire 契约，在桥上相遇）
fn style_of(s: WireBlankStyle) -> BlankStyle {
    match s {
        WireBlankStyle::Lines => BlankStyle::Lines,
        WireBlankStyle::Dots => BlankStyle::Dots,
        WireBlankStyle::Blank => BlankStyle::Blank,
    }
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

fn answer_block(q: &ExamQuestion, ctx: &Ctx, meta: BlockMeta) -> Option<AnswerBlock> {
    let lines = answer_lines(q, ctx.options);
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
fn answer_lines(q: &ExamQuestion, options: &ExportOptions) -> Vec<AnswerLine> {
    if !options.include_answer {
        return Vec::new();
    }
    if is_written(q.kind) && !q.structure_parts.is_empty() {
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

// ═══════════════════════════ 测试 ═══════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::model::{Callout, CalloutKind, ExamMeta, ExamOption, ExamSection};
    use crate::models::question_structure::AnalysisBlock;
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
        // 短题整块不跨页，也不许与它后面的答案离婚
        assert!(!q.meta.breakable);
        assert!(q.meta.keep_with_next);
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
}
