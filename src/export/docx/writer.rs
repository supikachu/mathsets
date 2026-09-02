//! docx 内容写入器（T2.7）— 实施计划 §5.4、§6.2
//!
//! `ExamBundle + ExportOptions` → OOXML 片段（`Package.body`）+ 媒体部件 + 页脚，
//! 最后交给 [`build`] 装成 .docx 字节。答案的放置只看 `options.answer_at_end`、Callout
//! 只看装配器已经过滤过的 `q.callouts` —— 与 `markdown.rs` 同一套门控，两种格式不得对
//! 同一份请求排出不同的内容。
//!
//! **单位**：OOXML 里长度三种单位并存 —— 版面用 twips（1pt = 20tw）、图片用 EMU
//! （1cm = 360000）、选项列数判定用 em（[`choice_grid`] 的口径，1em = 正文字号 10.5pt 的
//! 方块字宽）。换算常量只在本文件顶部出现一次。
//!
//! **降级优先于中断**：一条公式转不出 → 红色等宽原文 + 一条 `Issue`；一张图抓不到或
//! docx 不支持（SVG/WebP）→ 占位文字 + 一条 `Issue`。整卷绝不因一处坏内容失败（§5.3）。
//!
//! **子元素顺序**：`w:pPr`、`w:tblPr`、`w:tcPr`、`w:trPr`、`w:rPr` 都有 schema 顺序，
//! Word 宽容、WPS 不容，而 M2 的验收标准是两者都能打开 —— 顺序照抄 Word 自身产出。

use std::collections::HashMap;
use std::path::Path;

use quick_xml::escape::escape;

use super::{CT_FOOTER, ExtraPart, ExtraRel, NS_PIC, NS_W, Package, a4_sect_pr, build, ns_decl};
use crate::export::assets::{FetchedImage, fetch_image};
use crate::export::content::split_content;
use crate::export::markdown::{collect_bundle_images, fmt_score};
use crate::export::math::{MathOutcome, omml::to_omml, to_mathml};
use crate::export::model::{
    Callout, CalloutKind, ExamBundle, ExamOption, ExamQuestion, ExamSection, ExportOptions,
    ImageAlign, InlineImage, InlineNode, Issue, IssueField, IssueSeverity, QuestionKind,
    TableAlign,
};
use crate::models::question_structure::{QuestionPart, walk_leaves};
use crate::typeset::blocks::choice_grid;

// ── 版面常量（twips：1pt = 20tw）──

/// A4 正文可用宽度：页宽 11906 − 左右边距 1418×2，与 [`super::A4_SECT_PR`] 同源
const TEXT_TWIPS: f64 = 9070.0;
/// 题号悬挂缩进 = 选项表格缩进，与 styles.xml 的 `QuestionNo` 一致
const INDENT_TWIPS: f64 = 420.0;
/// 1em：正文 10.5pt 的方块字宽
const EM_TWIPS: f64 = 210.0;
/// 选项栅格的可用栏宽（em）—— 交给 [`choice_grid::decide`]，与它的 em 口径同源
const GRID_EM: f64 = (TEXT_TWIPS - INDENT_TWIPS) / EM_TWIPS;
/// 问树每深一层多缩进的量
const PART_STEP_TWIPS: i64 = 280;

// ── 图片单位换算 ──

/// 1cm 的 EMU
const CM_EMU: f64 = 360_000.0;
/// 96dpi 下 1px 的厘米数（编辑器给的 `width` 按屏幕像素理解）
const PX_CM: f64 = 2.54 / 96.0;
/// 图片宽度上限（cm）
const MAX_IMAGE_CM: f64 = 14.0;
/// 图片高度上限（cm）：约等于一页正文高，长图按比例整体缩
const MAX_IMAGE_H_CM: f64 = 24.0;
/// docx 能直嵌的格式（`[Content_Types].xml` 只 Default 了这几种扩展名）
const EMBEDDABLE: &[&str] = &["png", "jpg", "jpeg", "gif"];

// ── 段落格式片段（`w:pPr` 子元素顺序：pStyle → keepNext → pBdr → shd → spacing → ind → jc）──

/// 题号段（样式已带 keepNext + 悬挂缩进）
const PPR_QUESTION: &str = r#"<w:pPr><w:pStyle w:val="QuestionNo"/></w:pPr>"#;
/// 选项单元格段：去段后距、固定行距，避免网格里高低不齐
const PPR_CHOICE: &str = concat!(
    r#"<w:pPr><w:pStyle w:val="Choice"/>"#,
    r#"<w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr>"#,
);
/// 内嵌答案/解析段：与题面文字对齐，且不与所解释的题分两页
const PPR_PLAIN: &str = r#"<w:pPr><w:keepNext/><w:ind w:left="420"/></w:pPr>"#;
/// 卷末答案/解析条目：悬挂缩进，续行与编号后的文字对齐
const PPR_TAIL: &str =
    r#"<w:pPr><w:ind w:left="420" w:hanging="420"/><w:jc w:val="left"/></w:pPr>"#;
/// 大题说明与考试说明行
const PPR_NOTE: &str = r#"<w:pPr><w:keepNext/><w:ind w:left="420"/><w:jc w:val="left"/></w:pPr>"#;
/// 相邻两张表之间必须有一个段落，否则 OOXML 读者会把两张表读成一张
const SPACER: &str =
    r#"<w:p><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/></w:pPr></w:p>"#;

// ── 字符格式片段（`w:rPr` 顺序：rFonts → b → bCs → color → sz → szCs）──

const RPR_TITLE: &str = concat!(
    r#"<w:rPr><w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:eastAsia="黑体"/>"#,
    r#"<w:b/><w:bCs/><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr>"#,
);
const RPR_SUBTITLE: &str =
    r#"<w:rPr><w:rFonts w:eastAsia="楷体"/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr>"#;
const RPR_BOLD: &str = "<w:rPr><w:b/><w:bCs/></w:rPr>";
const RPR_SMALL: &str = "<w:rPr><w:sz w:val=\"18\"/><w:szCs w:val=\"18\"/></w:rPr>";
/// 降级公式：红色等宽，纸上显眼但不影响阅读，教师据此回编辑器改源
const RPR_DEGRADED: &str = concat!(
    r#"<w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas" w:cs="Consolas"/>"#,
    r#"<w:color w:val="C00000"/></w:rPr>"#,
);

/// 表格单元格内边距（twips）：不留左右距会让选项贴住格线
const TBL_CELL_MAR: &str = concat!(
    r#"<w:tblCellMar>"#,
    r#"<w:top w:w="30" w:type="dxa"/><w:left w:w="80" w:type="dxa"/>"#,
    r#"<w:bottom w:w="30" w:type="dxa"/><w:right w:w="80" w:type="dxa"/>"#,
    r#"</w:tblCellMar>"#,
);
/// 卷头与题干表格的边框（选项表故意无边框）
const TBL_BORDERS: &str = concat!(
    r#"<w:tblBorders>"#,
    r#"<w:top w:val="single" w:sz="4" w:space="0" w:color="808080"/>"#,
    r#"<w:left w:val="single" w:sz="4" w:space="0" w:color="808080"/>"#,
    r#"<w:bottom w:val="single" w:sz="4" w:space="0" w:color="808080"/>"#,
    r#"<w:right w:val="single" w:sz="4" w:space="0" w:color="808080"/>"#,
    r#"<w:insideH w:val="single" w:sz="4" w:space="0" w:color="808080"/>"#,
    r#"<w:insideV w:val="single" w:sz="4" w:space="0" w:color="808080"/>"#,
    r#"</w:tblBorders>"#,
);

/// 生成结果
pub struct DocxResult {
    /// .docx 字节（OPC 包）
    pub bytes: Vec<u8>,
    /// 生成期新问题（公式降级、图片跳过等；与题级 issues 合并进 X-Export-Warnings）
    pub issues: Vec<Issue>,
}

/// 生成 docx。图片抓取失败与公式转换失败都只记警告，不中断整卷。
pub async fn generate_docx(
    bundle: &ExamBundle,
    options: &ExportOptions,
    upload_dir: &Path,
) -> DocxResult {
    let mut w = Writer::new(bundle, options);
    w.prefetch(upload_dir).await;
    w.render();
    w.finish()
}

// ═══════════════════════════════ 写入器 ═══════════════════════════════

/// 一处内容的归属：Issue 要能指回题号与字段
#[derive(Debug, Clone, Copy)]
struct Slot {
    qno: Option<u32>,
    field: IssueField,
}

impl Slot {
    const fn new(qno: Option<u32>, field: IssueField) -> Self {
        Self { qno, field }
    }
}

/// 一张已登记进包里的图片
#[derive(Debug, Clone)]
struct ImagePart {
    /// `document.xml` 里指向该媒体部件的关系 Id
    rid: String,
    /// 固有像素尺寸
    nat: (u32, u32),
}

struct Writer<'a> {
    bundle: &'a ExamBundle,
    options: &'a ExportOptions,
    body: String,
    issues: Vec<Issue>,
    /// URL → 部件（`None` = 抓到了但不能嵌 / 抓取失败，已记 Issue；不再重试）
    images: HashMap<String, Option<ImagePart>>,
    media: Vec<(String, Vec<u8>)>,
    rels: Vec<ExtraRel>,
    /// 下一条自定义关系的 rId（`rId1`/`rId2` 已被 styles/settings 占用）
    next_rid: u32,
    /// `wp:docPr/@id` 计数器：同图复用同一 rId，但每次绘制都要独立 id
    next_draw: u32,
}

impl<'a> Writer<'a> {
    fn new(bundle: &'a ExamBundle, options: &'a ExportOptions) -> Self {
        Self {
            bundle,
            options,
            body: String::new(),
            issues: Vec::new(),
            images: HashMap::new(),
            media: Vec::new(),
            rels: Vec::new(),
            next_rid: 3,
            next_draw: 0,
        }
    }

    // ── 图片预取 ──

    /// 渲染前一次性抓图：这样 `render` 可以是同步的（问树递归不必写成 async 递归）
    async fn prefetch(&mut self, upload_dir: &Path) {
        let bundle = self.bundle;
        let options = self.options;
        for (qno, url) in collect_bundle_images(bundle, options) {
            if self.images.contains_key(&url) {
                continue;
            }
            let part = match fetch_image(&url, upload_dir).await {
                Ok(img) => self.register(qno, &url, img),
                Err(e) => {
                    self.issues.push(Issue {
                        question_no: qno,
                        field: IssueField::Image,
                        severity: IssueSeverity::Warning,
                        latex: None,
                        reason: format!("图片 {} 处理失败：{}", url, e),
                    });
                    None
                }
            };
            self.images.insert(url, part);
        }
    }

    /// 登记媒体部件；不支持的格式与读不出尺寸的图不入库（返回 `None`）
    fn register(&mut self, qno: Option<u32>, url: &str, img: FetchedImage) -> Option<ImagePart> {
        let ext = img.ext.to_ascii_lowercase();
        if !EMBEDDABLE.contains(&ext.as_str()) {
            self.issues.push(Issue {
                question_no: qno,
                field: IssueField::Image,
                severity: IssueSeverity::Warning,
                latex: None,
                reason: format!("图片 {url} 是 {ext}，docx 不嵌入（仅支持 PNG/JPEG/GIF）"),
            });
            return None;
        }
        let Some(nat) = image_size(&ext, &img.bytes) else {
            self.issues.push(Issue {
                question_no: qno,
                field: IssueField::Image,
                severity: IssueSeverity::Warning,
                latex: None,
                reason: format!("图片 {url} 读不出尺寸，已跳过"),
            });
            return None;
        };
        let rid = format!("rId{}", self.next_rid);
        self.next_rid += 1;
        let name = format!("media/image{}.{}", self.media.len() + 1, ext);
        self.rels.push(ExtraRel {
            id: rid.clone(),
            kind: "image".into(),
            target: name.clone(),
        });
        self.media.push((name, img.bytes));
        Some(ImagePart { rid, nat })
    }

    // ── 全卷渲染 ──

    fn render(&mut self) {
        self.header();
        let bundle = self.bundle;
        let sections: Vec<&ExamSection> = bundle.sections.iter().collect();
        for sec in &sections {
            self.section(sec);
        }
        let options = self.options;
        let questions: Vec<&ExamQuestion> = bundle
            .sections
            .iter()
            .flat_map(|s| s.questions.iter())
            .collect();
        if options.include_answer && options.answer_at_end {
            self.heading("参考答案");
            for q in &questions {
                self.answer_block(q, true);
            }
        }
        if options.include_analysis && options.answer_at_end {
            self.heading("试题解析");
            for q in &questions {
                self.analysis_block(q, true);
            }
        }
    }

    /// 收尾：表格保护段 + 页脚部件 + 打包
    fn finish(mut self) -> DocxResult {
        // body 以表格结尾时，表格会和 sectPr 之间的结构含混，补一个空段兜住
        if self.body.ends_with("</w:tbl>") {
            self.body.push_str(SPACER);
        }
        let footer_rid = format!("rId{}", self.next_rid);
        self.next_rid += 1;
        self.rels.push(ExtraRel {
            id: footer_rid.clone(),
            kind: "footer".into(),
            target: "footer1.xml".into(),
        });
        let body = std::mem::take(&mut self.body);
        let pkg = Package {
            title: self.bundle.title.clone(),
            body,
            sect_pr: a4_sect_pr(Some(&footer_rid)),
            extra_parts: vec![ExtraPart {
                name: "word/footer1.xml".into(),
                content_type: CT_FOOTER.into(),
                xml: footer_xml(),
            }],
            extra_rels: std::mem::take(&mut self.rels),
            media: std::mem::take(&mut self.media),
        };
        DocxResult {
            bytes: build(&pkg),
            issues: self.issues,
        }
    }

    // ── 卷头 ──

    fn header(&mut self) {
        let bundle = self.bundle;
        let meta = &bundle.exam_meta;
        let total = meta.total_score.unwrap_or_else(|| {
            bundle
                .sections
                .iter()
                .flat_map(|s| s.questions.iter())
                .map(|q| q.score)
                .sum()
        });

        self.push(&centered(Some(RPR_TITLE), &bundle.title));
        if let Some(s) = bundle.subtitle.as_deref().filter(|s| !s.trim().is_empty()) {
            self.push(&centered(Some(RPR_SUBTITLE), s));
        }
        let mut bits: Vec<String> = Vec::new();
        if let Some(d) = meta.duration {
            bits.push(format!("考试时间 {d} 分钟"));
        }
        bits.push(format!("满分 {} 分", fmt_score(total)));
        self.push(&centered(None, &bits.join("　　")));

        // 考生信息表：学校已知就填，班级/姓名/考号留空给考场手写
        let blank = "＿＿＿＿＿＿";
        let row = vec![
            format!(
                "学校：{}",
                meta.school
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(blank)
            ),
            format!("班级：{blank}"),
            format!("姓名：{blank}"),
            format!("考号：{blank}"),
        ];
        self.push(&plain_table(&[row], &split_twips(TEXT_TWIPS as i64, 4)));
        self.push(SPACER);

        // 分值汇总表：一列一大题，末列合计；第三行留给监考评分
        let sections: Vec<&ExamSection> = bundle.sections.iter().collect();
        if !sections.is_empty() {
            let mut keys = vec!["题号".to_string()];
            let mut scores = vec!["分值".to_string()];
            let mut blanks = vec!["得分".to_string()];
            let mut sum = 0.0_f64;
            for (i, sec) in sections.iter().enumerate() {
                keys.push(section_key(&sec.title, i));
                let s: f64 = sec.questions.iter().map(|q| q.score).sum();
                sum += s;
                scores.push(fmt_score(s));
                blanks.push(String::new());
            }
            keys.push("合计".into());
            scores.push(fmt_score(sum));
            blanks.push(String::new());
            let cols = keys.len();
            self.push(&plain_table(
                &[keys, scores, blanks],
                &split_twips(TEXT_TWIPS as i64, cols),
            ));
            self.push(SPACER);
        }

        if !meta.instructions.is_empty() {
            self.push(&paragraph_of(PPR_NOTE, &run("考试说明：", Some(RPR_BOLD))));
            for (i, text) in meta.instructions.iter().enumerate() {
                self.push(&paragraph_of(
                    PPR_NOTE,
                    &run(&format!("{}. {}", i + 1, text.trim()), Some(RPR_SMALL)),
                ));
            }
        }
    }

    fn push(&mut self, xml: &str) {
        self.body.push_str(xml);
    }

    // ── 大题与题 ──

    fn heading(&mut self, text: &str) {
        self.push(&paragraph_of(
            r#"<w:pPr><w:pStyle w:val="SectionTitle"/></w:pPr>"#,
            &run(text, None),
        ));
    }

    fn section(&mut self, sec: &ExamSection) {
        let count = sec.questions.len();
        let score: f64 = sec.questions.iter().map(|q| q.score).sum();
        self.heading(&format!(
            "{}（共 {count} 题 · {} 分）",
            sec.title,
            fmt_score(score)
        ));
        if let Some(ins) = sec.instruction.as_deref().filter(|t| !t.trim().is_empty()) {
            self.push(&paragraph_of(PPR_NOTE, &run(ins.trim(), Some(RPR_SMALL))));
        }
        for q in &sec.questions {
            self.question(q);
        }
    }

    /// 一道题：题号 + 分值 → 题干 → 选项网格 → 问树 → Callout → 按开关内嵌答案/解析
    fn question(&mut self, q: &ExamQuestion) {
        let slot = Slot::new(Some(q.number), IssueField::Stem);
        let lead = format!(
            "{}{}",
            run(&format!("{}. ", q.number), Some(RPR_BOLD)),
            run(&format!("（{} 分）", fmt_score(q.score)), Some(RPR_SMALL))
        );
        self.paragraph(PPR_QUESTION, &lead, &q.stem, slot);
        self.option_grid(q);
        if !q.structure_parts.is_empty() {
            self.parts(&q.structure_parts, 0, q.number);
        }
        for c in &q.callouts {
            self.callout(c, q.number);
        }
        let options = self.options;
        if options.include_answer && !options.answer_at_end {
            self.answer_block(q, false);
        }
        if options.include_analysis && !options.answer_at_end {
            self.analysis_block(q, false);
        }
    }

    /// 问树：逐层展开，每深一层多缩一段（叶子答案归答案区管）
    fn parts(&mut self, parts: &[QuestionPart], depth: usize, qno: u32) {
        let slot = Slot::new(Some(qno), IssueField::Structure);
        let ppr = part_ppr(depth);
        for p in parts {
            let lead = if p.label.is_empty() {
                String::new()
            } else {
                run(&format!("{} ", p.label), None)
            };
            let nodes = split_content(&p.stem);
            self.paragraph(&ppr, &lead, &nodes, slot);
            self.parts(&p.children, depth + 1, qno);
        }
    }

    /// 选项网格：列数只问 [`choice_grid::decide`] —— docx 与 typst 因此永远排同样的列数
    fn option_grid(&mut self, q: &ExamQuestion) {
        if q.options.is_empty() {
            return;
        }
        let cols = choice_grid::decide(&q.options, GRID_EM).columns.max(1);
        let row_twips = (TEXT_TWIPS - INDENT_TWIPS) as i64;
        let widths = split_twips(row_twips, cols);
        let slot = Slot::new(Some(q.number), IssueField::Choice);

        // 选项表：无边框、固定布局、与题面文字同缩进
        let mut tbl = format!(
            concat!(
                r#"<w:tbl><w:tblPr><w:tblW w:w="{row}" w:type="dxa"/>"#,
                r#"<w:tblInd w:w="{ind}" w:type="dxa"/>"#,
                r#"<w:tblLayout w:type="fixed"/>"#,
                "{mar}",
                r#"</w:tblPr><w:tblGrid>"#,
            ),
            row = row_twips,
            ind = INDENT_TWIPS as i64,
            mar = TBL_CELL_MAR
        );
        for wd in &widths {
            tbl.push_str(&format!(r#"<w:gridCol w:w="{wd}"/>"#));
        }
        tbl.push_str("</w:tblGrid>");

        for chunk in q.options.chunks(cols) {
            let mut row = String::new();
            for (i, opt) in chunk.iter().enumerate() {
                row.push_str(&self.option_cell(opt, widths[i], slot));
            }
            // 末行不满：补空格子。少给 tc 会让 Word 把列宽重新平分，栅格就歪了
            for wd in widths.iter().take(cols).skip(chunk.len()) {
                row.push_str(&format!(
                    concat!(
                        r#"<w:tc><w:tcPr><w:tcW w:w="{wd}" w:type="dxa"/>"#,
                        r#"</w:tcPr><w:p>{empty}</w:p></w:tc>"#
                    ),
                    wd = wd,
                    empty = PPR_CHOICE
                ));
            }
            tbl.push_str(&format!(
                concat!(r#"<w:tr><w:trPr><w:cantSplit/></w:trPr>"#, "{row}</w:tr>"),
                row = row
            ));
        }
        tbl.push_str("</w:tbl>");
        // 相邻两张表会被读者合并成一张（题干以表格收尾时就会撞上）：中间垫一个段
        if self.body.ends_with("</w:tbl>") {
            self.body.push_str(SPACER);
        }
        self.body.push_str(&tbl);
    }

    fn option_cell(&mut self, opt: &ExamOption, width: i64, slot: Slot) -> String {
        let lead = if opt.label.is_empty() {
            String::new()
        } else {
            run(&format!("{}. ", opt.label), None)
        };
        let mut inner = String::new();
        self.push_inline(&opt.content, PPR_CHOICE, &lead, slot, &mut inner);
        cell_xml(&inner, width)
    }

    // ── Callout ──

    fn callout(&mut self, c: &Callout, qno: u32) {
        let (border, fill) = palette(c.kind);
        let ppr = callout_ppr(&border, &fill);
        let slot = Slot::new(Some(qno), IssueField::Analysis);
        let lead = run(&format!("{} ", c.title.trim()), Some(RPR_BOLD));
        self.paragraph(&ppr, &lead, &c.nodes, slot);
    }

    // ── 答案与解析 ──

    /// 答案段：内嵌时首段挂「答案：」，卷末时首段挂题号
    fn answer_block(&mut self, q: &ExamQuestion, at_end: bool) {
        let items = answer_items(q);
        if items.is_empty() {
            return;
        }
        let slot = Slot::new(Some(q.number), IssueField::Answer);
        let ppr = if at_end { PPR_TAIL } else { PPR_PLAIN };
        for (i, (lead, nodes)) in items.iter().enumerate() {
            let mut pre = String::new();
            if at_end {
                if i == 0 {
                    pre.push_str(&run(&format!("{}. ", q.number), Some(RPR_BOLD)));
                }
            } else {
                pre.push_str(&run(
                    if i == 0 { "答案：" } else { "　　　" },
                    Some(RPR_BOLD),
                ));
            }
            if !lead.trim().is_empty() {
                pre.push_str(&run(&format!("{} ", lead.trim()), None));
            }
            self.paragraph(ppr, &pre, nodes, slot);
        }
    }

    fn analysis_block(&mut self, q: &ExamQuestion, at_end: bool) {
        let items = analysis_items(q);
        if items.is_empty() {
            return;
        }
        let slot = Slot::new(Some(q.number), IssueField::Analysis);
        let ppr = if at_end { PPR_TAIL } else { PPR_PLAIN };
        for (i, (lead, nodes)) in items.iter().enumerate() {
            let mut pre = String::new();
            if at_end {
                if i == 0 {
                    pre.push_str(&run(&format!("{}. ", q.number), Some(RPR_BOLD)));
                }
            } else {
                pre.push_str(&run(
                    if i == 0 { "解析：" } else { "　　　" },
                    Some(RPR_BOLD),
                ));
            }
            if !lead.trim().is_empty() {
                pre.push_str(&run(&format!("{} ", lead.trim()), Some(RPR_BOLD)));
            }
            self.paragraph(ppr, &pre, nodes, slot);
        }
    }

    // ── 行内节点 ──

    fn paragraph(&mut self, ppr: &str, lead: &str, nodes: &[InlineNode], slot: Slot) {
        let mut out = String::new();
        self.push_inline(nodes, ppr, lead, slot, &mut out);
        self.body.push_str(&out);
    }

    /// 行内节点 → 段落 XML。块级节点（display 公式、图片、图组、表格）会先收尾当前段落、
    /// 自己占一段/一张表，若后面还有行内内容再重开一个同样格式的段落。
    fn push_inline(
        &mut self,
        nodes: &[InlineNode],
        ppr: &str,
        lead: &str,
        slot: Slot,
        out: &mut String,
    ) {
        out.push_str(&format!("<w:p>{ppr}{lead}"));
        let mut open = true;
        let last = nodes.len().saturating_sub(1);
        for (i, node) in nodes.iter().enumerate() {
            let more = i < last;
            match node {
                InlineNode::Text { text } => out.push_str(&run(text, None)),
                InlineNode::LineBreak => out.push_str("<w:r><w:br/></w:r>"),
                InlineNode::Math {
                    latex,
                    display: false,
                } => match self.fragment(latex, false, slot) {
                    Fragment::Omml(f) => out.push_str(&f),
                    Fragment::Text(r) => out.push_str(&r),
                },
                InlineNode::Math {
                    latex,
                    display: true,
                } => {
                    let block = self.display_math(latex, ppr, slot);
                    close_and_emit(out, ppr, &mut open, more, &block);
                }
                InlineNode::Image {
                    alt,
                    url,
                    width,
                    align,
                } => {
                    let block = self.figure(alt.as_deref(), url, *width, *align);
                    close_and_emit(out, ppr, &mut open, more, &block);
                }
                InlineNode::ImgRow {
                    align,
                    images,
                    caption,
                } => {
                    let block = self.img_row(images, *align, caption.as_deref());
                    close_and_emit(out, ppr, &mut open, more, &block);
                }
                InlineNode::Table {
                    header,
                    aligns,
                    rows,
                } => {
                    let block = self.md_table(header, aligns, rows, slot);
                    close_and_emit(out, ppr, &mut open, more, &block);
                }
            }
        }
        if open {
            out.push_str("</w:p>");
        }
    }

    /// display 公式：`m:oMathPara` 由本 writer 负责包（R2 明确这不是转换器的事）
    fn display_math(&mut self, latex: &str, ppr: &str, slot: Slot) -> String {
        match self.fragment(latex, true, slot) {
            Fragment::Omml(f) => format!("<w:p>{ppr}<m:oMathPara>{f}</m:oMathPara></w:p>"),
            Fragment::Text(r) => format!("<w:p>{ppr}{r}</w:p>"),
        }
    }

    /// 公式：能转就出 OMML，转不出就地降级并记一条 Issue
    fn fragment(&mut self, latex: &str, display: bool, slot: Slot) -> Fragment {
        match omml_of(latex, display) {
            Ok(omml) => Fragment::Omml(omml),
            Err(reason) => {
                self.issues.push(Issue {
                    question_no: slot.qno,
                    field: slot.field,
                    severity: IssueSeverity::Warning,
                    latex: Some(latex.to_string()),
                    reason,
                });
                Fragment::Text(run(latex, Some(RPR_DEGRADED)))
            }
        }
    }

    // ── 图片 ──

    /// 块级单图：独占一段，`align` 决定段内对齐（缺省居中）
    fn figure(
        &mut self,
        alt: Option<&str>,
        url: &str,
        width: Option<u32>,
        align: Option<ImageAlign>,
    ) -> String {
        let ppr = image_ppr(align);
        let inner = self
            .drawing(url, width, alt)
            .unwrap_or_else(|| missing_image_run(url, alt));
        paragraph_of(&ppr, &inner)
    }

    /// 图组：一段内并排 + 图注段（图注小一号居中）
    fn img_row(
        &mut self,
        images: &[InlineImage],
        align: Option<ImageAlign>,
        caption: Option<&str>,
    ) -> String {
        let ppr = image_ppr(align);
        let mut row = String::new();
        for (i, img) in images.iter().enumerate() {
            if i > 0 {
                row.push_str(&run("　", None));
            }
            row.push_str(
                &self
                    .drawing(&img.url, img.width, img.alt.as_deref())
                    .unwrap_or_else(|| missing_image_run(&img.url, img.alt.as_deref())),
            );
        }
        let mut out = paragraph_of(&ppr, &row);
        if let Some(c) = caption.map(str::trim).filter(|c| !c.is_empty()) {
            out.push_str(&paragraph_of(
                r#"<w:pPr><w:jc w:val="center"/></w:pPr>"#,
                &run(c, Some(RPR_SMALL)),
            ));
        }
        out
    }

    /// 一次绘制。`wp:inline` 是「随文字流」的绘图，`wp:anchor` 才是浮动的 —— 只用前者
    fn drawing(&mut self, url: &str, width: Option<u32>, alt: Option<&str>) -> Option<String> {
        let part = match self.images.get(url) {
            Some(Some(p)) => p.clone(),
            _ => return None,
        };
        let (cx, cy) = extent(part.nat, width);
        self.next_draw += 1;
        let id = self.next_draw;
        let name = format!("图片 {id}");
        let descr = alt.unwrap_or_default();
        Some(format!(
            concat!(
                r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">"#,
                r#"<wp:extent cx="{cx}" cy="{cy}"/>"#,
                r#"<wp:effectExtent l="0" t="0" r="0" b="0"/>"#,
                r#"<wp:docPr id="{id}" name="{name}" descr="{descr}"/>"#,
                r#"<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>"#,
                r#"<a:graphic><a:graphicData uri="{pic}">"#,
                r#"<pic:pic><pic:nvPicPr>"#,
                r#"<pic:cNvPr id="{id}" name="{name}" descr="{descr}"/>"#,
                r#"<pic:cNvPicPr><a:picLocks noChangeAspect="1" noChangeArrowheads="1"/></pic:cNvPicPr>"#,
                r#"</pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:srcRect/>"#,
                r#"<a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
                r#"<pic:spPr bwMode="auto"><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>"#,
                r#"</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
            ),
            cx = cx,
            cy = cy,
            id = id,
            name = escape(&name),
            descr = escape(descr),
            rid = escape(&part.rid),
            pic = NS_PIC,
        ))
    }

    // ── 题干里的 Markdown 表格 ──

    fn md_table(
        &mut self,
        header: &[String],
        aligns: &[TableAlign],
        rows: &[Vec<String>],
        slot: Slot,
    ) -> String {
        let cols = header.len().max(1);
        let widths = split_twips(TEXT_TWIPS as i64, cols);
        let mut s = open_table(&widths);
        let mut head_row = String::new();
        for (i, cell) in header.iter().enumerate() {
            let ppr = cell_ppr(aligns.get(i).copied().unwrap_or(TableAlign::Center));
            head_row.push_str(&cell_xml(&self.cell_body(cell, &ppr, slot), widths[i]));
        }
        s.push_str(&format!(
            concat!(
                r#"<w:tr><w:trPr><w:cantSplit/><w:tblHeader/></w:trPr>"#,
                "{cells}</w:tr>"
            ),
            cells = head_row
        ));
        for row in rows {
            let mut cells = String::new();
            for (i, &wd) in widths.iter().enumerate() {
                let text = row.get(i).map(String::as_str).unwrap_or("");
                let ppr = cell_ppr(aligns.get(i).copied().unwrap_or(TableAlign::Left));
                cells.push_str(&cell_xml(&self.cell_body(text, &ppr, slot), wd));
            }
            s.push_str(&format!(
                concat!(r#"<w:tr><w:trPr><w:cantSplit/></w:trPr>"#, "{cells}</w:tr>"),
                cells = cells
            ));
        }
        s.push_str("</w:tbl>");
        s
    }

    /// 单元格正文：单元格里的公式同样要变 OMML，所以走完整的行内渲染
    fn cell_body(&mut self, text: &str, ppr: &str, slot: Slot) -> String {
        let nodes = split_content(text);
        let mut out = String::new();
        self.push_inline(&nodes, ppr, "", slot, &mut out);
        out
    }
}

// ═══════════════════════════════ 片段与换算 ═══════════════════════════════

/// 块级内容插进正在写的段落序列：先收尾当前段落，插块，后面还有行内内容就重开
fn close_and_emit(out: &mut String, ppr: &str, open: &mut bool, more: bool, block: &str) {
    if *open {
        out.push_str("</w:p>");
        *open = false;
    }
    out.push_str(block);
    if more {
        out.push_str(&format!("<w:p>{ppr}"));
        *open = true;
    }
}

enum Fragment {
    /// 可直接进段落的 OMML 片段
    Omml(String),
    /// 降级：已成形的 run（红色等宽原文）
    Text(String),
}

/// LaTeX → OMML。两级转换的失败原因都直接作为 Issue 的 reason
fn omml_of(latex: &str, display: bool) -> Result<String, String> {
    let mathml = match to_mathml(latex, display) {
        MathOutcome::Ok(m) => m,
        MathOutcome::Failed(reason) => return Err(reason),
    };
    match to_omml(&mathml) {
        MathOutcome::Ok(omml) => Ok(omml),
        MathOutcome::Failed(reason) => Err(reason),
    }
}

/// 一个文字 run：`\n` → `w:br`、`\t` → `w:tab`，其余进 `w:t`（保留首尾空格）
fn run(text: &str, rpr: Option<&str>) -> String {
    let mut s = String::from("<w:r>");
    if let Some(p) = rpr {
        s.push_str(p);
    }
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            s.push_str("<w:br/>");
        }
        for (j, seg) in line.split('\t').enumerate() {
            if j > 0 {
                s.push_str("<w:tab/>");
            }
            if !seg.is_empty() {
                s.push_str(&format!(
                    r#"<w:t xml:space="preserve">{}</w:t>"#,
                    escape(seg)
                ));
            }
        }
    }
    s.push_str("</w:r>");
    s
}

fn paragraph_of(ppr: &str, lead: &str) -> String {
    format!("<w:p>{ppr}{lead}</w:p>")
}

fn centered(rpr: Option<&str>, text: &str) -> String {
    paragraph_of(
        r#"<w:pPr><w:spacing w:after="60"/><w:jc w:val="center"/></w:pPr>"#,
        &run(text, rpr),
    )
}

fn part_ppr(depth: usize) -> String {
    let left = INDENT_TWIPS as i64 + PART_STEP_TWIPS * depth as i64;
    format!(r#"<w:pPr><w:keepNext/><w:ind w:left="{left}"/></w:pPr>"#)
}

fn image_ppr(align: Option<ImageAlign>) -> String {
    let jc = match align.unwrap_or(ImageAlign::Center) {
        ImageAlign::Left => "left",
        ImageAlign::Center => "center",
        ImageAlign::Right => "right",
    };
    // keepNext：图与紧随其后的文字不许分页分开
    format!(r#"<w:pPr><w:keepNext/><w:jc w:val="{jc}"/></w:pPr>"#)
}

/// 表格单元格段：清掉样式带来的缩进，只留对齐（加粗走 run 级，由调用方决定）
fn cell_ppr(align: TableAlign) -> String {
    let jc = match align {
        TableAlign::Left => "left",
        TableAlign::Center => "center",
        TableAlign::Right => "right",
    };
    format!(
        concat!(
            r#"<w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/>"#,
            r#"<w:ind w:left="0" w:hanging="0" w:firstLine="0"/><w:jc w:val="{jc}"/></w:pPr>"#,
        ),
        jc = jc
    )
}

/// 四类 Callout 的边框色 / 底纹色（与前端语义色同一取向）
fn palette(kind: CalloutKind) -> (String, String) {
    let (border, fill) = match kind {
        CalloutKind::Knowledge => ("2E74B5", "E8F0FA"),
        CalloutKind::ErrorProne => ("C00000", "FBE9E7"),
        CalloutKind::Tip => ("548235", "EDF6E8"),
        CalloutKind::Approach => ("7030A0", "F2EAF9"),
    };
    (border.into(), fill.into())
}

/// 提示框段落：直接格式覆盖样式的灰边框（四类各自的配色）
fn callout_ppr(border: &str, fill: &str) -> String {
    let side = |which: &str| {
        format!(
            r#"<w:{which} w:val="single" w:sz="4" w:space="4" w:color="{border}"/>"#,
            which = which
        )
    };
    format!(
        concat!(
            r#"<w:pPr><w:pStyle w:val="Callout"/><w:keepNext/><w:pBdr>"#,
            "{top}{left}{bottom}{right}",
            r#"</w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="{fill}"/></w:pPr>"#,
        ),
        top = side("top"),
        left = side("left"),
        bottom = side("bottom"),
        right = side("right"),
        fill = escape(fill)
    )
}

/// 等分栏宽（twips），余数给最后一列，保证 tblGrid 与各 tcW 之和一致
fn split_twips(total: i64, cols: usize) -> Vec<i64> {
    let cols = cols.max(1) as i64;
    let base = total / cols;
    let mut v = vec![base; cols as usize];
    if let Some(last) = v.last_mut() {
        *last += total - base * cols;
    }
    v
}

fn open_table(widths: &[i64]) -> String {
    let total: i64 = widths.iter().sum();
    let mut s = format!(
        concat!(
            r#"<w:tbl><w:tblPr><w:tblW w:w="{total}" w:type="dxa"/>"#,
            "{borders}",
            r#"<w:tblLayout w:type="fixed"/>{mar}</w:tblPr><w:tblGrid>"#,
        ),
        total = total,
        borders = TBL_BORDERS,
        mar = TBL_CELL_MAR
    );
    for wd in widths {
        s.push_str(&format!(r#"<w:gridCol w:w="{wd}"/>"#));
    }
    s.push_str("</w:tblGrid>");
    s
}

fn cell_xml(body: &str, width: i64) -> String {
    // ECMA-376 §17.4.5.7：单元格最后一个块级元素必须是段落。选项里带 Markdown 表格时
    // body 会以 </w:tbl> 收尾，不补空段就是坏文件（表现同漏声明 Content-Type）。
    let tail = if body.ends_with("</w:tbl>") {
        SPACER
    } else {
        ""
    };
    format!(
        concat!(
            r#"<w:tc><w:tcPr><w:tcW w:w="{width}" w:type="dxa"/>"#,
            r#"<w:vAlign w:val="center"/></w:tcPr>{body}{tail}</w:tc>"#,
        ),
        width = width,
        body = body,
        tail = tail
    )
}

/// 卷头用的纯文字表格（信息表 / 分值表）：单元格一律居中
fn plain_table(rows: &[Vec<String>], widths: &[i64]) -> String {
    let mut s = open_table(widths);
    for row in rows {
        let mut cells = String::new();
        for (i, text) in row.iter().enumerate() {
            let ppr = r#"<w:pPr><w:spacing w:after="0"/><w:jc w:val="center"/></w:pPr>"#;
            cells.push_str(&cell_xml(
                &paragraph_of(ppr, &run(text, None)),
                widths.get(i).copied().unwrap_or(1_000),
            ));
        }
        s.push_str(&format!("<w:tr>{cells}</w:tr>"));
    }
    s.push_str("</w:tbl>");
    s
}

/// 抓不到 / 不支持的图：纸上留一段红字占位，题号与地址都在，教师一眼能定位
fn missing_image_run(url: &str, alt: Option<&str>) -> String {
    let label = alt.map(str::trim).filter(|a| !a.is_empty()).unwrap_or(url);
    run(&format!("[图片缺失：{label}]"), Some(RPR_DEGRADED))
}

/// 目标尺寸（EMU）：`width` 按 96dpi 折算，超出宽/高上限时等比缩到框内
fn extent(nat: (u32, u32), want_px: Option<u32>) -> (u32, u32) {
    let (nw, nh) = (f64::from(nat.0.max(1)), f64::from(nat.1.max(1)));
    let mut w = want_px.unwrap_or(nat.0.max(1)) as f64 * PX_CM;
    if !w.is_finite() || w <= 0.0 {
        w = nw * PX_CM;
    }
    let mut h = w * nh / nw;
    if w > MAX_IMAGE_CM {
        let k = MAX_IMAGE_CM / w;
        w *= k;
        h *= k;
    }
    if h > MAX_IMAGE_H_CM {
        let k = MAX_IMAGE_H_CM / h;
        w *= k;
        h *= k;
    }
    ((w * CM_EMU).round() as u32, (h * CM_EMU).round() as u32)
}

/// 分值表的大题列名：优先用标题里的「一、二、…」前缀，否则退化成序号
///
/// PDF 侧的首页分值汇总表（`typeset::typst_gen`）共用这一份口径：同一张卷子的两种格式，
/// 列名不许一边是「一」一边是「1」。
pub(crate) fn section_key(title: &str, idx: usize) -> String {
    let head = title.trim().split(['、', '.', ' ']).next().unwrap_or("");
    let usable = head.chars().count() <= 4
        && !head.is_empty()
        && !head.chars().next().is_some_and(|c| c.is_ascii_digit());
    if usable {
        head.into()
    } else {
        (idx + 1).to_string()
    }
}

/// 答案条目：解答题按问树叶子逐条，其余按空分隔（与 markdown 的口径一致）
fn answer_items(q: &ExamQuestion) -> Vec<(String, Vec<InlineNode>)> {
    if q.kind == QuestionKind::Solution && !q.structure_parts.is_empty() {
        return walk_leaves(&q.structure_parts)
            .iter()
            .filter_map(|p| {
                p.answer
                    .as_deref()
                    .map(str::trim)
                    .filter(|a| !a.is_empty())
                    .map(|a| (p.label.clone(), split_content(a)))
            })
            .collect();
    }
    if q.answers.is_empty() {
        return Vec::new();
    }
    let mut nodes: Vec<InlineNode> = Vec::new();
    for (i, a) in q.answers.iter().enumerate() {
        if i > 0 {
            nodes.push(InlineNode::Text {
                text: "；".to_string(),
            });
        }
        nodes.extend(split_content(a));
    }
    vec![(String::new(), nodes)]
}

/// 解析条目：题级解法块 + 问树叶子各解法块（标题拼法与 markdown 一致）
fn analysis_items(q: &ExamQuestion) -> Vec<(String, Vec<InlineNode>)> {
    let mut out: Vec<(String, Vec<InlineNode>)> = Vec::new();
    for blk in &q.analyses {
        if blk.content.trim().is_empty() {
            continue;
        }
        out.push((blk.title.clone(), split_content(&blk.content)));
    }
    for leaf in walk_leaves(&q.structure_parts) {
        for blk in &leaf.analyses {
            if blk.content.trim().is_empty() {
                continue;
            }
            let title = if leaf.label.is_empty() {
                blk.title.clone()
            } else if blk.title.trim().is_empty() {
                leaf.label.clone()
            } else {
                format!("{} {}", leaf.label, blk.title)
            };
            out.push((title, split_content(&blk.content)));
        }
    }
    out
}

/// 页脚：`第 { PAGE } 页 共 { NUMPAGES } 页`。不写域字符就只是两个数字，会被当成手写文本；
/// 缓存值 1 是占位，Word/WPS 打开与打印时自会重算。
fn footer_xml() -> String {
    let txt = |t: &str| format!(r#"<w:r>{RPR_SMALL}<w:t xml:space="preserve">{t}</w:t></w:r>"#);
    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>",
            r#"<w:ftr {w}>"#,
            r#"<w:p><w:pPr><w:spacing w:after="0"/><w:jc w:val="center"/></w:pPr>"#,
            "{a}{page}{b}{total}{c}",
            "</w:p></w:ftr>",
        ),
        w = ns_decl("w", NS_W),
        a = txt("第 "),
        page = field("PAGE", "1"),
        b = txt(" 页 共 "),
        total = field("NUMPAGES", "1"),
        c = txt(" 页"),
    )
}

/// 一个域：begin → instrText → separate → 缓存结果 → end
fn field(instr: &str, cached: &str) -> String {
    format!(
        concat!(
            r#"<w:r><w:fldChar w:fldCharType="begin"/></w:r>"#,
            r#"<w:r><w:instrText xml:space="preserve"> {instr} </w:instrText></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="separate"/></w:r>"#,
            r#"<w:r><w:t>{cached}</w:t></w:r>"#,
            r#"<w:r><w:fldChar w:fldCharType="end"/></w:r>"#,
        ),
        instr = escape(instr),
        cached = escape(cached),
    )
}

// ═══════════════════════════════ 图片固有尺寸（零新依赖）═══════════════════════════════

const PNG_SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// 图片像素尺寸：PNG 读 IHDR、JPEG 扫 SOF、GIF 读逻辑屏幕描述符
///
/// 只需要尺寸就能等比换算高度，为此挂一个完整解码器不值当。
fn image_size(ext: &str, bytes: &[u8]) -> Option<(u32, u32)> {
    match ext {
        "png" => png_size(bytes),
        "jpg" | "jpeg" => jpeg_size(bytes),
        "gif" => gif_size(bytes),
        _ => None,
    }
}

fn positive(w: u32, h: u32) -> Option<(u32, u32)> {
    if w > 0 && h > 0 { Some((w, h)) } else { None }
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]) as u32)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([
        *b.get(at)?,
        *b.get(at + 1)?,
        *b.get(at + 2)?,
        *b.get(at + 3)?,
    ]))
}

fn le16(b: &[u8], at: usize) -> Option<u32> {
    Some(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?]) as u32)
}

fn png_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 24 || &b[..8] != PNG_SIG || &b[12..16] != b"IHDR" {
        return None;
    }
    positive(be32(b, 16)?, be32(b, 20)?)
}

fn gif_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 10 || &b[..3] != b"GIF" {
        return None;
    }
    positive(le16(b, 6)?, le16(b, 8)?)
}

fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return None;
    }
    let mut i = 2usize;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            i += 1; // 填充/噪声字节
            continue;
        }
        let marker = b[i + 1];
        // SOFn（跳过 DHT 等占用的编号）：帧头里就带尺寸
        if matches!(marker, 0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF) {
            // 段长 2B（i+2）· 精度 1B（i+4）· 高 2B（i+5）· 宽 2B（i+7）
            return positive(be16(b, i + 7)?, be16(b, i + 5)?);
        }
        if marker == 0x01 || matches!(marker, 0xD0..=0xD9) {
            i += 2; // 独立标记（含 EOI/SOS），无长度字段
            continue;
        }
        let len = be16(b, i + 2)? as usize;
        if len < 2 {
            return None;
        }
        i += 2 + len; // 带长度的段：整段跳过
    }
    None
}

// ═══════════════════════════════ 单元测试 ═══════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::docx::test_support::{
        Parts, assert_opc_invariants, parse, part, text_of, unzip,
    };
    use crate::export::docx::{NS_A, NS_M};
    use crate::export::model::{ExamMeta, ExportMode};
    use crate::models::question_structure::AnalysisBlock;

    // ── 构造 ──

    fn t(text: &str) -> InlineNode {
        InlineNode::Text { text: text.into() }
    }
    fn math(latex: &str, display: bool) -> InlineNode {
        InlineNode::Math {
            latex: latex.into(),
            display,
        }
    }
    fn image(url: &str, width: Option<u32>) -> InlineNode {
        InlineNode::Image {
            alt: None,
            url: url.into(),
            width,
            align: None,
        }
    }
    /// 单元格内容按原始文本二次处理（`$y=x$` 在格子里也要变 OMML）
    fn table_node() -> InlineNode {
        InlineNode::Table {
            header: vec!["x".into(), "y".into()],
            aligns: vec![TableAlign::Center, TableAlign::Left],
            rows: vec![vec!["1".into(), "$y=x$".into()]],
        }
    }
    fn options(bodies: [&str; 4]) -> Vec<ExamOption> {
        bodies
            .iter()
            .enumerate()
            .map(|(i, b)| ExamOption {
                label: char::from(b'A' + i as u8).to_string(),
                content: vec![t(b)],
            })
            .collect()
    }
    fn question(number: u32, stem: Vec<InlineNode>) -> ExamQuestion {
        ExamQuestion {
            number,
            score: 5.0,
            kind: QuestionKind::SingleChoice,
            stem,
            options: options(["1", "2", "3", "4"]),
            answers: vec!["A".into()],
            analyses: vec![],
            structure_parts: vec![],
            callouts: vec![],
            answer_space: None,
            issues: vec![],
        }
    }
    fn bundle(questions: Vec<ExamQuestion>) -> ExamBundle {
        ExamBundle {
            title: "集合单元测验".into(),
            subtitle: Some("必修一".into()),
            exam_meta: ExamMeta {
                school: Some("实验中学".into()),
                duration: Some(90),
                total_score: None,
                instructions: vec!["闭卷作答".into()],
            },
            mode: ExportMode::Exam,
            sections: vec![ExamSection {
                title: "一、单选题".into(),
                instruction: Some("每题 5 分".into()),
                questions,
            }],
        }
    }
    fn one(q: ExamQuestion) -> ExamBundle {
        bundle(vec![q])
    }

    /// 生成 → 解压 → 三条 OPC 不变量：每个用例都顺带证明「这文件 Word 打不打得开」
    async fn render(b: &ExamBundle, o: &ExportOptions) -> (DocxResult, Parts) {
        render_in(b, o, Path::new("target/no-such-upload-dir")).await
    }
    async fn render_in(b: &ExamBundle, o: &ExportOptions, dir: &Path) -> (DocxResult, Parts) {
        let r = generate_docx(b, o, dir).await;
        let parts = unzip(&r.bytes);
        assert_opc_invariants(&parts);
        (r, parts)
    }

    fn document(parts: &Parts) -> roxmltree::Document<'static> {
        parse(parts, "word/document.xml")
    }
    fn xml_of(parts: &Parts, name: &str) -> String {
        text_of(part(parts, name))
    }
    fn count(doc: &roxmltree::Document, ns: &str, tag: &str) -> usize {
        doc.descendants()
            .filter(|n| n.has_tag_name((ns, tag)))
            .count()
    }
    /// 纸上看得见的文字（`w:t` 拼接；公式文字在 `m:t` 里，不计）
    fn body_text(doc: &roxmltree::Document) -> String {
        doc.descendants()
            .filter(|n| n.has_tag_name((NS_W, "t")))
            .map(|n| n.text().unwrap_or_default())
            .collect()
    }
    /// 每张表：`(栅格列数, 各行单元格数)`
    fn table_stats(doc: &roxmltree::Document) -> Vec<(usize, Vec<usize>)> {
        doc.descendants()
            .filter(|n| n.has_tag_name((NS_W, "tbl")))
            .map(|tbl| {
                let cols = tbl
                    .children()
                    .find(|n| n.has_tag_name((NS_W, "tblGrid")))
                    .map(|g| {
                        g.children()
                            .filter(|n| n.has_tag_name((NS_W, "gridCol")))
                            .count()
                    })
                    .unwrap_or(0);
                let rows = tbl
                    .children()
                    .filter(|n| n.has_tag_name((NS_W, "tr")))
                    .map(|tr| {
                        tr.children()
                            .filter(|n| n.has_tag_name((NS_W, "tc")))
                            .count()
                    })
                    .collect();
                (cols, rows)
            })
            .collect()
    }

    /// 真 PNG：只用到 image 的 png 编码特性
    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        image::DynamicImage::new_rgba8(w, h)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .expect("编码 PNG");
        out
    }

    // ── 选项栅格 ──

    #[tokio::test]
    async fn option_grid_shapes_follow_choice_width() {
        for (body, cols, rows) in [
            ("1", 4usize, 1usize),
            ("下列说法中正确的是哪一个", 2, 2),
            ("经检验下列四个选项中只有一个是符合题目要求的选项内容", 1, 4),
        ] {
            let mut q = question(1, vec![t("题干")]);
            q.options = options([body; 4]);
            let (_, parts) = render(&one(q), &ExportOptions::default()).await;
            let doc = document(&parts);
            let stats = table_stats(&doc);
            assert_eq!(stats.len(), 3, "卷头两张 + 选项一张");
            let (grid, cells) = &stats[2];
            assert_eq!(*grid, cols, "「{body}」应排 {cols} 列");
            assert_eq!(cells.len(), rows, "「{body}」应排 {rows} 行");
            assert!(
                cells.iter().all(|c| *c == cols),
                "末行不满要补空格子，否则 Word 会重平分列宽：{cells:?}"
            );
        }
    }

    #[tokio::test]
    async fn option_table_follows_the_question_paragraph() {
        let (_, parts) = render(
            &one(question(1, vec![t("题干")])),
            &ExportOptions::default(),
        )
        .await;
        let doc = document(&parts);
        assert!(
            doc.descendants().any(|tbl| {
                tbl.has_tag_name((NS_W, "tbl"))
                    && tbl.prev_sibling().is_some_and(|p| {
                        p.is_element()
                            && p.has_tag_name((NS_W, "p"))
                            && p.descendants().any(|c| {
                                c.has_tag_name((NS_W, "pStyle"))
                                    && c.attribute("val") == Some("QuestionNo")
                            })
                    })
            }),
            "选项表必须紧跟题号段（R5 探针按这个结构排）"
        );
        // 防腰斩链落在样式上：题号段/大题标题/提示框都带 keepNext + keepLines
        let styles = xml_of(&parts, "word/styles.xml");
        assert_eq!(styles.matches(r#"<w:keepNext/><w:keepLines/>"#).count(), 3);
    }

    // ── 公式 ──

    #[tokio::test]
    async fn display_math_gets_one_mathpara() {
        let q = question(
            1,
            vec![
                t("已知 "),
                math("x^2", false),
                t(" 与 "),
                math(r"\frac{1}{2}", true),
            ],
        );
        let (r, parts) = render(&one(q), &ExportOptions::default()).await;
        assert!(r.issues.is_empty(), "{:?}", r.issues);
        let doc = document(&parts);
        assert_eq!(count(&doc, NS_M, "oMath"), 2, "一条公式一个 m:oMath");
        assert_eq!(
            count(&doc, NS_M, "oMathPara"),
            1,
            "只有 display 那条外套 oMathPara"
        );
        assert!(
            !body_text(&doc).contains("frac"),
            "公式不得以 LaTeX 原文留在纸上"
        );
    }

    #[tokio::test]
    async fn bad_math_degrades_without_failing_the_paper() {
        let q = question(
            1,
            vec![
                t("题干 "),
                math(r"\frac{1}{", false),
                t(" 与 "),
                math("y^2", false),
            ],
        );
        let (r, parts) = render(&one(q), &ExportOptions::default()).await;
        assert_eq!(r.issues.len(), 1, "{:?}", r.issues);
        let issue = &r.issues[0];
        assert_eq!(issue.question_no, Some(1));
        assert_eq!(issue.field, IssueField::Stem);
        assert_eq!(issue.severity, IssueSeverity::Warning);
        assert_eq!(issue.latex.as_deref(), Some(r"\frac{1}{"));
        let doc = document(&parts);
        assert_eq!(count(&doc, NS_M, "oMath"), 1, "坏公式降级，好公式照常");
        assert!(
            body_text(&doc).contains(r"\frac{1}{"),
            "降级要印出原文，教师据此回编辑器改源"
        );
        assert!(xml_of(&parts, "word/document.xml").contains(r#"<w:color w:val="C00000"/>"#));
    }

    // ── 图片 ──

    #[tokio::test]
    async fn images_embed_skip_and_warn() {
        let dir = std::env::temp_dir().join(format!("mathset-docx-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(dir.join("q")).unwrap();
        let png = png_bytes(800, 600);
        std::fs::write(dir.join("q/a.png"), &png).unwrap();
        std::fs::write(
            dir.join("q/b.svg"),
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="10"></svg>"#,
        )
        .unwrap();
        std::fs::write(dir.join("q/c.webp"), b"RIFF\0\0\0\0WEBPVP8 ").unwrap();

        let q = question(
            1,
            vec![
                image("/uploads/q/a.png", Some(400)),
                image("/uploads/q/b.svg", None),
                image("/uploads/q/c.webp", None),
                image("/uploads/q/gone.png", None),
            ],
        );
        let (r, parts) = render_in(&one(q), &ExportOptions::default(), &dir).await;

        let media: Vec<&str> = parts
            .iter()
            .map(|(n, _)| n.as_str())
            .filter(|n| n.starts_with("word/media/"))
            .collect();
        assert_eq!(media, ["word/media/image1.png"], "只有 PNG/JPEG/GIF 进包");
        assert_eq!(
            part(&parts, "word/media/image1.png"),
            &png[..],
            "字节不得被改写"
        );
        let warns: Vec<&Issue> = r
            .issues
            .iter()
            .filter(|i| i.field == IssueField::Image)
            .collect();
        assert_eq!(warns.len(), 3, "SVG / WebP / 缺失各一条：{:?}", r.issues);

        let doc = document(&parts);
        assert_eq!(count(&doc, NS_A, "blip"), 1);
        assert_eq!(count(&doc, NS_W, "drawing"), 1);
        assert_eq!(body_text(&doc).matches("[图片缺失").count(), 3);
        assert!(
            xml_of(&parts, "word/document.xml").contains(r#"cx="3810000" cy="2857500""#),
            "400px@96dpi 等比 800×600 应为 10.58×7.94cm"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn extent_converts_px_and_caps_both_axes() {
        let cm = |emu: u32| emu as f64 / CM_EMU;
        let (cx, cy) = extent((800, 600), Some(400));
        assert!((cm(cx) - 10.5833).abs() < 1e-3 && (cm(cy) - 7.9375).abs() < 1e-3);
        let (cx, cy) = extent((800, 600), Some(2000));
        assert!((cm(cx) - 14.0).abs() < 1e-3, "宽度上限 14cm");
        assert!((cm(cy) - 10.5).abs() < 1e-3, "缩放不得改变纵横比");
        let (cx, cy) = extent((100, 1000), None);
        assert!((cm(cy) - 24.0).abs() < 1e-3, "长图按高度上限整体缩");
        assert!((cm(cx) - 2.4).abs() < 1e-3);
        assert_eq!(
            extent((200, 100), None),
            (1_905_000, 952_500),
            "无 width 用固有像素"
        );
        assert!(extent((0, 0), Some(0)).0 > 0, "零尺寸也不能出 0 extent");
    }

    #[test]
    fn intrinsic_sizes_come_from_headers() {
        assert_eq!(image_size("png", &png_bytes(37, 91)), Some((37, 91)));
        assert_eq!(image_size("gif", b"GIF89a\x25\x00\x5B\x00"), Some((37, 91)));
        assert_eq!(
            image_size(
                "jpg",
                &[
                    0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x00, 0x64, 0x00, 0x78
                ]
            ),
            Some((120, 100))
        );
        assert_eq!(image_size("svg", b"<svg/>"), None);
        assert_eq!(image_size("png", &[0, 1, 2]), None);
    }

    // ── 答案与解析位置 ──

    #[tokio::test]
    async fn answer_and_analysis_follow_the_two_switches() {
        let mut q = question(1, vec![t("题干")]);
        q.analyses = vec![AnalysisBlock {
            id: "a".into(),
            title: "思路".into(),
            content: "数形结合".into(),
        }];
        let b = one(q);

        let (_, parts) = render(&b, &ExportOptions::default()).await;
        let text = body_text(&document(&parts));
        assert!(text.contains("参考答案") && text.contains("1. A"), "{text}");
        assert!(!text.contains("答案："), "卷末模式不许把答案内嵌到题下");

        let mut inline = ExportOptions::default();
        inline.answer_at_end = false;
        inline.include_analysis = true;
        let (_, parts) = render(&b, &inline).await;
        let text = body_text(&document(&parts));
        assert!(
            text.contains("答案：A") && text.contains("解析：思路 数形结合"),
            "{text}"
        );
        assert!(!text.contains("参考答案") && !text.contains("试题解析"));

        let mut bare = ExportOptions::default();
        bare.include_answer = false;
        let (_, parts) = render(&b, &bare).await;
        let text = body_text(&document(&parts));
        assert!(!text.contains("答案") && !text.contains("解析"), "{text}");
    }

    // ── 卷头与页脚 ──

    #[tokio::test]
    async fn header_carries_info_and_score_tables() {
        let mut q1 = question(1, vec![t("甲")]);
        let mut q2 = question(2, vec![t("乙")]);
        q1.options.clear();
        q2.options.clear();
        q2.score = 3.5;
        let (_, parts) = render(&bundle(vec![q1, q2]), &ExportOptions::default()).await;
        let doc = document(&parts);
        assert_eq!(
            table_stats(&doc),
            vec![(4, vec![4]), (3, vec![3, 3, 3])],
            "考生信息表 4 列 1 行 / 分值表 3 列 3 行"
        );
        let text = body_text(&doc);
        for want in [
            "集合单元测验",
            "必修一",
            "考试时间 90 分钟",
            "满分 8.5 分",
            "学校：实验中学",
            "班级：＿＿＿＿＿＿",
            "姓名：＿＿＿＿＿＿",
            "考号：＿＿＿＿＿＿",
            "题号",
            "一",
            "分值",
            "得分",
            "合计",
            "8.5",
            "1. 闭卷作答",
            "每题 5 分",
            "一、单选题（共 2 题 · 8.5 分）",
        ] {
            assert!(text.contains(want), "卷头缺「{want}」：{text}");
        }
    }

    #[tokio::test]
    async fn footer_page_fields_are_wired() {
        let (_, parts) = render(
            &one(question(1, vec![t("题干")])),
            &ExportOptions::default(),
        )
        .await;
        let ftr = xml_of(&parts, "word/footer1.xml");
        assert!(
            ftr.contains(r#"<w:instrText xml:space="preserve"> PAGE </w:instrText>"#),
            "{ftr}"
        );
        assert!(ftr.contains("NUMPAGES"), "{ftr}");

        let doc = xml_of(&parts, "word/document.xml");
        let sect = &doc[doc.find("<w:sectPr>").unwrap()..];
        assert!(
            sect.contains(r#"<w:footerReference w:type="default" r:id="rId3"/>"#),
            "{sect}"
        );
        assert!(
            sect.find("footerReference").unwrap() < sect.find("<w:pgSz").unwrap(),
            "footerReference 必须排在 pgSz 之前（schema 顺序）"
        );
        let rels = xml_of(&parts, "word/_rels/document.xml.rels");
        assert!(
            rels.contains(r#"Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml""#),
            "{rels}"
        );
    }

    // ── 整卷结构 ──

    #[tokio::test]
    async fn rich_paper_keeps_cell_and_style_invariants() {
        let mut q1 = question(
            1,
            vec![
                t("如图，"),
                math("a+b", false),
                math(r"\begin{cases}x>0\\y<0\end{cases}", true),
                image("/uploads/pic.png", Some(120)),
                table_node(),
                t("，则"),
            ],
        );
        // 选项里嵌表格：必然单列，且单元格以嵌套表收尾
        q1.options = options(["表", "2", "3", "4"]);
        q1.options[0].content = vec![table_node()];
        q1.callouts = vec![
            Callout {
                kind: CalloutKind::Knowledge,
                title: "考点".into(),
                nodes: vec![t("集合的运算"), math(r"A\cap B", false)],
            },
            Callout {
                kind: CalloutKind::ErrorProne,
                title: "易错".into(),
                nodes: vec![t("漏掉空集")],
            },
        ];

        let mut q2 = question(2, vec![t("解不等式")]);
        q2.kind = QuestionKind::Solution;
        q2.options.clear();
        q2.structure_parts = vec![QuestionPart {
            id: "p1".into(),
            label: "(1)".into(),
            stem: "若 $x>0$，求 $x$".into(),
            children: vec![QuestionPart {
                id: "p1a".into(),
                label: "①".into(),
                stem: "再讨论".into(),
                children: vec![],
                answer: Some("x=1".into()),
                analyses: vec![AnalysisBlock {
                    id: "al".into(),
                    title: "解法一".into(),
                    content: "代入检验".into(),
                }],
                no_analysis_needed: false,
                label_dirty: false,
            }],
            answer: Some("x>0".into()),
            analyses: vec![],
            no_analysis_needed: false,
            label_dirty: false,
        }];

        let mut opts = ExportOptions::default();
        opts.include_analysis = true;
        opts.answer_at_end = false;
        let (r, parts) = render(&bundle(vec![q1, q2]), &opts).await;
        // 除那张不存在的图，全卷零降级
        assert!(
            r.issues
                .iter()
                .all(|i| i.field == IssueField::Image && i.severity == IssueSeverity::Warning),
            "{:?}",
            r.issues
        );

        let doc = document(&parts);
        assert_eq!(
            count(&doc, NS_M, "oMath"),
            7,
            "题干/选项表格/Callout/问树里的公式都要出来"
        );
        assert_eq!(count(&doc, NS_M, "oMathPara"), 1);
        for tc in doc.descendants().filter(|n| n.has_tag_name((NS_W, "tc"))) {
            let last = tc.children().filter(|n| n.is_element()).last();
            assert!(
                last.is_some_and(|n| n.has_tag_name((NS_W, "p"))),
                "单元格必须以段落收尾（嵌表后补空段）"
            );
        }
        let styles = parse(&parts, "word/styles.xml");
        for n in doc
            .descendants()
            .filter(|n| n.has_tag_name((NS_W, "pStyle")))
        {
            let id = n.attribute("val").unwrap_or_default();
            assert!(
                styles
                    .descendants()
                    .any(|s| s.has_tag_name((NS_W, "style")) && s.attribute("styleId") == Some(id)),
                "document.xml 用了未定义样式 {id}"
            );
        }
        let text = body_text(&doc);
        for want in [
            "(1) 若",
            "① 再讨论",
            "解法一",
            "考点",
            "易错",
            "答案：",
            "解析：",
        ] {
            assert!(text.contains(want), "缺「{want}」：{text}");
        }
        // 图片缺失时纸上留占位、段落带 keepNext（图与下文不分页）
        assert!(text.contains("[图片缺失"));
        assert!(
            xml_of(&parts, "word/document.xml")
                .contains(r#"<w:pPr><w:keepNext/><w:jc w:val="center"/></w:pPr>"#)
        );
    }

    /// ⛔ R5 决策门探针：`w:keepNext` 段落 + 紧随其后的 `w:tbl` 到底分不分页
    ///
    /// 夹具刻意做「矮」而不是「高」：每题 1 行题号 + 22 行选项 ≈ 26 行，一页（约 50 行）放得下
    /// 两道，分页边界因此落在题目中间。反过来若每题表格比一页还高，整块只会被推到新的一页，
    /// 每一对都没被考到，「0 违例」不作数。
    ///
    /// 判定靠**对照实验**：`python scripts/strip_keepnext.py` 剥掉全部 `w:keepnext` 产出同结构
    /// 副本，`scripts/check_keepnext.ps1` 一次跑完两份并按「题号段页码 != 首行起始页码」计违例。
    /// 压力由对照组直接给出，不用行高几何推算 —— Word 的 `Rows.Item(1).Height` 对 auto 行高返回
    /// 9999999、行尾标记被报回行的起始页码，几何量算出来的「受压」恒为 0。
    ///
    /// **2026-09-02 实测**（24 对，A4 / 10.5pt）：Word 2016 与 WPS 均为「探针 0/24 违例、
    /// 对照 8/24 违例」（对照里被孤立的题号段停在页尾 y≈717，选项首行去下一页）→ keepNext 在
    /// `w:tbl` 之前确实生效，两端一致 → **选项栅格保留 `w:tbl`**，不退回 `w:tabs`。
    ///
    /// 跑法：`DOCX_PROBE_TALL=22 cargo test --lib export::docx -- --ignored`
    #[tokio::test]
    #[ignore = "R5 决策门与 M2 手工验收时跑（需本机 Word / WPS）"]
    async fn writes_keepnext_probe_docx() {
        let path = std::env::var("DOCX_PROBE_PATH")
            .unwrap_or_else(|_| "target/t27_keepnext_probe.docx".into());
        let lines: usize = std::env::var("DOCX_PROBE_TALL")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(22);
        let tall = (1..=lines)
            .map(|i| format!("第 {i} 行 选项内容占位"))
            .collect::<Vec<_>>()
            .join("\n");
        let questions = (1..=24u32)
            .map(|n| {
                let mut q = question(
                    n,
                    vec![t(
                        "题干若干文字，用于把题号段与选项表之间的边界撑到分页附近。",
                    )],
                );
                q.options = options([tall.as_str(), "B", "C", "D"]);
                q
            })
            .collect();
        let b = bundle(questions);
        let r = generate_docx(&b, &ExportOptions::default(), Path::new("./uploads")).await;
        assert_opc_invariants(&unzip(&r.bytes));
        std::fs::write(&path, &r.bytes).expect("探针 docx 可写入");
        println!(
            "keepnext_probe={path} lines={lines} bytes={}",
            r.bytes.len()
        );
    }
}
