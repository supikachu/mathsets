//! OOXML `.docx` 打包（T2.6）
//!
//! docx 就是一个 OPC 包（zip + 若干 XML 部件）。本模块负责**容器与静态部件**：
//! `[Content_Types].xml`、`_rels/.rels`、`word/_rels/document.xml.rels`、`word/styles.xml`、
//! `word/settings.xml`、`docProps/*`，以及 `word/document.xml` 的外壳（根元素命名空间 +
//! 末尾 `sectPr`）。往里灌内容的是 `docx/writer.rs`（T2.7）。
//!
//! 打包复用 `zip = "2"`（与 Markdown bundle 同源，计划 §十三）。**部件名与关系是 Word 最
//! 挑剔的地方**，所以单测不只断言字符串，而是把产物重新解压后校验三条不变量：
//!
//! 1. 每个 XML 部件都是良构 XML；
//! 2. `[Content_Types].xml` 覆盖包内**每一个**部件（Default 扩展名或 Override 路径）——
//!    漏一个部件的表现是「Word 说文件已损坏」，而不是少显示一块内容；
//! 3. 每条 `Relationship/@Target` 都解析到包里真实存在的部件（相对路径按 `_rels` 所在
//!    目录的上一级解析，`word/_rels/document.xml.rels` 的目标相对 `word/`）。
//!
//! 静态部件的属性与子元素顺序按 Word 自身产出文件照抄：OOXML 的 `w:pPr`、`w:styles`、
//! `w:settings` 都有 schema 顺序，Word 宽容，WPS 与严格校验器不容 —— 而 M2 的验收标准是
//! 两者都能打开。

pub mod writer;

use quick_xml::escape::escape;
use std::io::Write;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::typeset::spec::LayoutSpec;

/// 主命名空间（WordprocessingML）
pub const NS_W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
/// 关系命名空间（`r:id`、`r:embed`）
pub const NS_R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
/// Office Math（OMML）命名空间
pub const NS_M: &str = "http://schemas.openxmlformats.org/officeDocument/2006/math";
/// Drawing 元素（`w:drawing` 里的 `wp:inline`）
pub const NS_WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
/// DrawingML 基础元素（`a:graphic` / `a:blip`）
pub const NS_A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
/// 图片元素（`pic:pic`）
pub const NS_PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

/// 页脚部件的 Content-Type（`ExtraPart::content_type` 用）
pub const CT_FOOTER: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";

const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const NS_CT: &str = "http://schemas.openxmlformats.org/package/2006/content-types";
const NS_REL: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const REL_BASE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";

/// 根元素上的 `xmlns:{prefix}="{uri}"` 声明
///
/// 只能运行时拼：`concat!` 接受字面量而不接受 `const` 路径，把 URI 抄两份进字面量则会让
/// 「根元素声明的命名空间」与「`NS_*` 常量」失去同源 —— 后者是单测判定文档根元素的依据。
fn ns_decl(prefix: &str, uri: &str) -> String {
    format!(r#"xmlns:{prefix}="{uri}""#)
}

/// mm → twips（1in = 25.4mm = 1440tw，四舍五入）
pub fn mm_to_twips(mm: f32) -> i64 {
    (f64::from(mm) * 1440.0 / 25.4).round() as i64
}

/// 页眉 / 页脚距纸边的距离（mm）：`LayoutSpec` 里没有这两个字段，沿用 Word 的常用口径
const HEADER_MM: f32 = 15.0;
const FOOTER_MM: f32 = 17.5;
/// CJK 行网格：与纸张无关，保持 M2 的取值
const DOC_GRID: &str = r#"<w:docGrid w:type="lines" w:linePitch="312"/>"#;

/// 页面设置（T4.12 / R11）：纸张、边距、装订位、栏数全部由 `LayoutSpec` 现算
///
/// 与 PDF 同源的三处映射：`w:pgSz` 取 `paper.size_mm()`（宽>高时补 `w:orient="landscape"`，
/// Word 不会自己转方向）；`w:pgMar` 的**生效**左边距 = `w:left` + `w:gutter`，正好对上
/// `LayoutSpec::margin_left_mm()`，所以 `Left` 装订带能原样搬过来；`w:cols/@w:space` 取
/// `column_gutter_mm()`。正文与表格的宽度不在这里，由 writer 侧按 `column_width_mm()` 现算。
///
/// 三处对不上是 Word 的模型里没有这些东西，不是漏做（R11）：`w:cols` 是平衡栏且页码按**张**计，
/// 与 R4「A3 对折半张 = 一页」差一倍；`CenterFold` 的折痕带无处安放（`w:gutter` 只认装订侧），
/// 退成普通栏距；竖排密封线不做（纯 XML 生成下文本框旋转不稳）。
///
/// `w:footerReference` **不能追加在末尾**：`sectPr` 的子元素顺序里 headerReference /
/// footerReference 排在 `pgSz` 之前（schema 顺序），排在后面时 Word 容忍、WPS 会整个忽略页脚。
pub fn sect_pr(spec: &LayoutSpec, footer_rid: Option<&str>) -> String {
    let (w, h) = spec.paper.size_mm();
    let (pw, ph) = (mm_to_twips(w as f32), mm_to_twips(h as f32));
    let orient = if pw > ph {
        r#" w:orient="landscape""#
    } else {
        ""
    };
    let refs = match footer_rid {
        None => String::new(),
        Some(rid) => format!(
            r#"<w:footerReference w:type="default" r:id="{}"/>"#,
            escape(rid)
        ),
    };
    format!(
        concat!(
            r#"<w:sectPr>{}<w:pgSz w:w="{}" w:h="{}"{}/>"#,
            r#"<w:pgMar w:top="{}" w:right="{}" w:bottom="{}" w:left="{}" "#,
            r#"w:header="{}" w:footer="{}" w:gutter="{}"/>"#,
            r#"<w:cols w:num="{}" w:space="{}"/>{}</w:sectPr>"#
        ),
        refs,
        pw,
        ph,
        orient,
        mm_to_twips(spec.margins.top_mm),
        mm_to_twips(spec.margins.right_mm),
        mm_to_twips(spec.margins.bottom_mm),
        // 声明值：装订带另记在 w:gutter，两者相加才是 PDF 侧那个生效左边距
        mm_to_twips(spec.margins.left_mm),
        mm_to_twips(HEADER_MM),
        mm_to_twips(FOOTER_MM),
        mm_to_twips(spec.binding_gutter_mm()),
        spec.columns.max(1),
        mm_to_twips(spec.column_gutter_mm()),
        DOC_GRID,
    )
}

/// 额外 XML 部件（页脚等）：路径、Content-Type Override、正文
///
/// 媒体部件只需 [`Package::media`]（扩展名已在 `[Content_Types].xml` 里 Default 掉），
/// 但 `word/footer1.xml` 必须有精确的 Override —— 缺它时 WPS 直接判文件损坏。
pub struct ExtraPart {
    /// 包内完整路径，如 `word/footer1.xml`
    pub name: String,
    /// 该部件的 Content-Type（写进 `[Content_Types].xml` 的 Override）
    pub content_type: String,
    /// 已含 XML 声明的部件正文
    pub xml: String,
}

/// `document.xml` 的一条自定义关系
///
/// 关系类型必须随目标一起给出：图片是 `…/relationships/image`、页脚是 `…/relationships/footer`，
/// 全按 image 写会让页脚关系失效（Word 静默不显示页脚，不报错）。
pub struct ExtraRel {
    /// 关系 Id，如 `rId5`
    pub id: String,
    /// 类型后缀，拼在 `…/officeDocument/2006/relationships/` 之后：`image` / `footer` / `header`
    pub kind: String,
    /// 相对 `word/` 的目标路径，如 `media/image1.png`、`footer1.xml`
    pub target: String,
}

/// 一个待打包的 docx
pub struct Package {
    /// 试卷标题（写进 docProps，也是 Word 窗口标题）
    pub title: String,
    /// `<w:body>` 内的全部内容（不含 `sectPr`）
    pub body: String,
    /// 页面设置，一般传 [`sect_pr`]（纸张、边距、装订、栏数由 `LayoutSpec` 现算）
    pub sect_pr: String,
    /// 额外 XML 部件及其 Content-Type 声明
    pub extra_parts: Vec<ExtraPart>,
    /// `document.xml` 的额外关系：图片与页脚用，见 [`ExtraRel`]
    ///
    /// `rId1` / `rId2` 已被 styles / settings 占用，自定义关系从 `rId3` 起。
    pub extra_rels: Vec<ExtraRel>,
    /// 媒体部件：`(相对 word/ 的路径, 字节)`，如 `("media/image1.png", …)`
    pub media: Vec<(String, Vec<u8>)>,
}

impl Package {
    /// 纯文字与公式的包：[`LayoutSpec::default()`]（A4 单栏）版面、无额外部件
    ///
    /// 页脚与图片由调用方追加字段。
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            sect_pr: sect_pr(&LayoutSpec::default(), None),
            extra_parts: Vec::new(),
            extra_rels: Vec::new(),
            media: Vec::new(),
        }
    }
}

/// 组装 docx 字节
pub fn build(pkg: &Package) -> Vec<u8> {
    let mut parts: Vec<(String, Vec<u8>)> = vec![
        // [Content_Types].xml 与 _rels/.rels 先行：OPC 读者靠这两份决定后面部件怎么解释
        (
            "[Content_Types].xml".into(),
            content_types_xml(&pkg.extra_parts).into_bytes(),
        ),
        ("_rels/.rels".into(), root_rels_xml().into_bytes()),
        (
            "word/document.xml".into(),
            document_xml(&pkg.body, &pkg.sect_pr).into_bytes(),
        ),
        (
            "word/_rels/document.xml.rels".into(),
            document_rels_xml(&pkg.extra_rels).into_bytes(),
        ),
        ("word/styles.xml".into(), styles_xml().into_bytes()),
        ("word/settings.xml".into(), settings_xml().into_bytes()),
        (
            "docProps/core.xml".into(),
            core_xml(&pkg.title).into_bytes(),
        ),
        ("docProps/app.xml".into(), app_xml().into_bytes()),
    ];
    for (name, bytes) in &pkg.media {
        parts.push((format!("word/{name}"), bytes.clone()));
    }
    for p in &pkg.extra_parts {
        parts.push((p.name.clone(), p.xml.clone().into_bytes()));
    }

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in parts {
        zip.start_file(&name, opts).expect("部件名合法");
        zip.write_all(&bytes).expect("写内存 zip 不会失败");
    }
    zip.finish().expect("收尾 zip").into_inner()
}

// ═══════════════════════════════ 部件 ═══════════════════════════════

/// 部件类型声明：`Default` 按扩展名、`Override` 按部件路径，必须覆盖全包
///
/// 调用方追加的 XML 部件（页脚等）在这里补 Override —— 漏掉的表现不是少一块内容，
/// 而是「Word 说文件已损坏」，所以 [`build`] 一并写出、单测按全包复查。
fn content_types_xml(extra: &[ExtraPart]) -> String {
    let mut s = format!(
        concat!(
            "{d}",
            r#"<Types xmlns="{ct}">"#,
            r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
            r#"<Default Extension="xml" ContentType="application/xml"/>"#,
            // 图片扩展名一次声明齐，加媒体部件时不必回写本部件（SVG 按计划在 docx 里跳过）
            r#"<Default Extension="png" ContentType="image/png"/>"#,
            r#"<Default Extension="jpg" ContentType="image/jpeg"/>"#,
            r#"<Default Extension="jpeg" ContentType="image/jpeg"/>"#,
            r#"<Default Extension="gif" ContentType="image/gif"/>"#,
            r#"<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>"#,
            r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#,
            r#"<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>"#,
            r#"<Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/>"#,
            r#"<Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/>"#,
        ),
        ct = NS_CT,
        d = XML_DECL
    );
    for p in extra {
        s.push_str(&format!(
            r#"<Override PartName="/{name}" ContentType="{ty}"/>"#,
            name = escape(&p.name),
            ty = escape(&p.content_type)
        ));
    }
    s.push_str("</Types>");
    s
}

/// 包级关系：入口文档 + 两份文档属性
fn root_rels_xml() -> String {
    format!(
        concat!(
            "{d}",
            r#"<Relationships xmlns="{rel}">"#,
            r#"<Relationship Id="rId1" Type="{base}/officeDocument" Target="word/document.xml"/>"#,
            r#"<Relationship Id="rId2" Type="{rel}/metadata/core-properties" Target="docProps/core.xml"/>"#,
            r#"<Relationship Id="rId3" Type="{base}/extended-properties" Target="docProps/app.xml"/>"#,
            r#"</Relationships>"#,
        ),
        rel = NS_REL,
        base = REL_BASE,
        d = XML_DECL
    )
}

/// 文档级关系：样式、设置，再加调用方传入的图片与页脚关系
fn document_rels_xml(extra: &[ExtraRel]) -> String {
    let mut s = format!(
        concat!(
            "{d}",
            r#"<Relationships xmlns="{rel}">"#,
            r#"<Relationship Id="rId1" Type="{base}/styles" Target="styles.xml"/>"#,
            r#"<Relationship Id="rId2" Type="{base}/settings" Target="settings.xml"/>"#,
        ),
        rel = NS_REL,
        base = REL_BASE,
        d = XML_DECL
    );
    for r in extra {
        s.push_str(&format!(
            r#"<Relationship Id="{id}" Type="{base}/{kind}" Target="{target}"/>"#,
            id = escape(&r.id),
            base = REL_BASE,
            kind = escape(&r.kind),
            target = escape(&r.target)
        ));
    }
    s.push_str("</Relationships>");
    s
}

/// 主文档部件：根元素声明 writer 会用到的全部前缀（含 `m:`，OMML 片段自身也带 `xmlns:m`）
fn document_xml(body: &str, sect_pr: &str) -> String {
    let mut s = String::with_capacity(body.len() + 512);
    s.push_str(XML_DECL);
    s.push_str(&format!(
        concat!(r#"<w:document {w} {r} {m} {wp} {a} {pic}>"#,),
        w = ns_decl("w", NS_W),
        r = ns_decl("r", NS_R),
        m = ns_decl("m", NS_M),
        wp = ns_decl("wp", NS_WP),
        a = ns_decl("a", NS_A),
        pic = ns_decl("pic", NS_PIC)
    ));
    s.push_str("<w:body>");
    s.push_str(body);
    s.push_str(sect_pr);
    s.push_str("</w:body></w:document>");
    s
}

/// 样式表：Normal 宋体 + Times New Roman 10.5pt，另加大题标题 / 题号 / 选项 / 提示框
///
/// `w:pPr` 子元素顺序按 schema：`keepNext → keepLines → pBdr → shd → spacing → ind → jc`，
/// 顺序错了 Word 容忍、WPS 可能整段丢格式。
fn styles_xml() -> String {
    format!(
        concat!(
            "{d}",
            r#"<w:styles {w}>"#,
            // ── 全局默认：正文中宋体 / 西文 Times New Roman / 10.5pt（sz 为半磅）
            r#"<w:docDefaults><w:rPrDefault><w:rPr>"#,
            r#"<w:rFonts w:ascii="Times New Roman" w:hAnsi="Times New Roman" w:eastAsia="宋体" w:cs="Times New Roman"/>"#,
            r#"<w:sz w:val="21"/><w:szCs w:val="21"/>"#,
            r#"<w:lang w:val="en-US" w:eastAsia="zh-CN" w:bidi="ar-SA"/>"#,
            r#"</w:rPr></w:rPrDefault>"#,
            r#"<w:pPrDefault><w:pPr><w:spacing w:after="0" w:line="276" w:lineRule="auto"/></w:pPr></w:pPrDefault>"#,
            r#"</w:docDefaults>"#,
            // ── 正文：两端对齐
            r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal">"#,
            r#"<w:name w:val="Normal"/><w:qFormat/><w:pPr><w:jc w:val="both"/></w:pPr></w:style>"#,
            // ── 大题标题：黑体小四加粗 + 灰底，且不与其下的小题分页
            r#"<w:style w:type="paragraph" w:styleId="SectionTitle">"#,
            r#"<w:name w:val="big question title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/>"#,
            r#"<w:pPr><w:keepNext/><w:keepLines/>"#,
            r#"<w:shd w:val="clear" w:color="auto" w:fill="E7E6E6"/>"#,
            r#"<w:spacing w:before="240" w:after="120"/><w:jc w:val="left"/></w:pPr>"#,
            r#"<w:rPr><w:rFonts w:eastAsia="黑体"/><w:b/><w:sz w:val="24"/><w:szCs w:val="24"/></w:rPr></w:style>"#,
            // ── 题号段：悬挂缩进（首行题号顶格，续行与题面文字对齐）
            r#"<w:style w:type="paragraph" w:styleId="QuestionNo">"#,
            r#"<w:name w:val="question no"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/>"#,
            r#"<w:pPr><w:keepNext/><w:keepLines/><w:spacing w:after="60"/><w:ind w:left="420" w:hanging="420"/>"#,
            r#"<w:jc w:val="left"/></w:pPr></w:style>"#,
            // ── 选项段：选项网格里逐格套用
            r#"<w:style w:type="paragraph" w:styleId="Choice">"#,
            r#"<w:name w:val="choice"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/>"#,
            r#"<w:pPr><w:spacing w:after="0"/><w:ind w:left="113"/><w:jc w:val="left"/></w:pPr></w:style>"#,
            // ── 提示框：四边框 + 浅底纹；四类 Callout 的配色由 writer 用直接格式覆盖
            r#"<w:style w:type="paragraph" w:styleId="Callout">"#,
            r#"<w:name w:val="callout"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/>"#,
            r#"<w:pPr><w:keepNext/><w:keepLines/><w:pBdr>"#,
            r#"<w:top w:val="single" w:sz="4" w:space="4" w:color="BFBFBF"/>"#,
            r#"<w:left w:val="single" w:sz="4" w:space="4" w:color="BFBFBF"/>"#,
            r#"<w:bottom w:val="single" w:sz="4" w:space="4" w:color="BFBFBF"/>"#,
            r#"<w:right w:val="single" w:sz="4" w:space="4" w:color="BFBFBF"/>"#,
            r#"</w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="F2F2F2"/>"#,
            r#"<w:spacing w:before="60" w:after="120"/><w:ind w:left="113" w:right="113"/>"#,
            r#"<w:jc w:val="left"/></w:pPr>"#,
            r#"<w:rPr><w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr></w:style>"#,
            r#"</w:styles>"#,
        ),
        d = XML_DECL,
        w = ns_decl("w", NS_W)
    )
}

/// 文档设置。`m:mathPr` 决定 Word 解释 OMML 的默认参数（字体 Cambria Math 等）；
/// 缺它公式仍能显示与编辑，但进公式编辑器会看到默认值跑偏。
fn settings_xml() -> String {
    format!(
        concat!(
            "{d}",
            r#"<w:settings {w} {m}>"#,
            r#"<w:zoom w:percent="100"/>"#,
            r#"<w:defaultTabStop w:val="420"/>"#,
            r#"<w:characterSpacingControl w:val="compressPunctuation"/>"#,
            r#"<m:mathPr><m:mathFont m:val="Cambria Math"/>"#,
            // brkBinSub 的字面量 `--` 是 Word 自己写出的值，不是占位符
            r#"<m:brkBin m:val="before"/><m:brkBinSub m:val="--"/><m:smallFrac m:val="0"/>"#,
            r#"<m:dispDef/><m:lMargin m:val="0"/><m:rMargin m:val="0"/>"#,
            r#"<m:defJc m:val="centerGroup"/><m:wrapIndent m:val="1440"/>"#,
            r#"<m:intLim m:val="subSup"/><m:naryLim m:val="undOvr"/></m:mathPr>"#,
            r#"<w:themeFontLang w:val="en-US" w:eastAsia="zh-CN"/>"#,
            r#"<w:clrSchemeMapping w:accent1="accent1" w:accent2="accent2" w:accent3="accent3" "#,
            r#"w:accent4="accent4" w:accent5="accent5" w:accent6="accent6" w:bg1="light1" "#,
            r#"w:bg2="light2" w:fg1="text1" w:fg2="text2" w:hlink="hyperlink" "#,
            r#"w:followedHyperlink="followedHyperlink"/>"#,
            r#"</w:settings>"#,
        ),
        d = XML_DECL,
        w = ns_decl("w", NS_W),
        m = ns_decl("m", NS_M)
    )
}

/// 核心属性：标题与时间戳（用户可控输入必须转义）
fn core_xml(title: &str) -> String {
    let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    format!(
        concat!(
            "{d}",
            r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" "#,
            r#"xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" "#,
            r#"xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">"#,
            r#"<dc:title>{title}</dc:title><dc:subject>试卷导出</dc:subject>"#,
            r#"<dc:creator>mathset</dc:creator><cp:lastModifiedBy>mathset</cp:lastModifiedBy>"#,
            r#"<dcterms:created xsi:type="dcterms:W3CDTF">{now}</dcterms:created>"#,
            r#"<dcterms:modified xsi:type="dcterms:W3CDTF">{now}</dcterms:modified>"#,
            r#"</cp:coreProperties>"#,
        ),
        title = escape(title),
        now = escape(&now),
        d = XML_DECL
    )
}

/// 扩展属性：生成器标识
fn app_xml() -> String {
    format!(
        concat!(
            "{d}",
            r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties">"#,
            "<Application>mathset</Application><AppVersion>16.0000</AppVersion></Properties>",
        ),
        d = XML_DECL
    )
}

// ═══════════════════════════════ 测试 ═══════════════════════════════

/// 测试公用：把产物重新解压再断言结构
///
/// 放成 `pub(crate)` 兄弟模块而不是塞进下面的 `mod tests`，是因为 `writer.rs`（T2.7）要做
/// 完全同一批断言 —— 复制两份「解包 + 三条不变量」，最容易漂移的就正是不变量本身。
#[cfg(test)]
pub(crate) mod test_support {
    use std::io::Read;

    pub type Parts = Vec<(String, Vec<u8>)>;

    /// 解压成 名字 → 内容
    pub fn unzip(bytes: &[u8]) -> Parts {
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("产物必须是合法 zip");
        let mut out = Parts::new();
        for i in 0..archive.len() {
            let mut f = archive.by_index(i).expect("可读条目");
            let mut buf = Vec::new();
            f.read_to_end(&mut buf).expect("可读内容");
            out.push((f.name().to_string(), buf));
        }
        out
    }

    pub fn text_of(bytes: &[u8]) -> String {
        String::from_utf8(bytes.to_vec()).expect("部件一律 UTF-8")
    }

    pub fn part<'p>(parts: &'p Parts, name: &str) -> &'p [u8] {
        &parts
            .iter()
            .find(|(n, _)| n == name)
            .unwrap_or_else(|| panic!("缺少部件 {name}"))
            .1
    }

    /// 包内哪些部件是 XML（媒体字节按二进制原样搬运，不得当 markup 解析）
    pub fn is_xml_part(name: &str) -> bool {
        name.ends_with(".xml") || name.ends_with(".rels")
    }

    /// 解析某个部件。roxmltree 要求引用比 Document 活得久，测试里 leak 一份最省事
    pub fn parse(parts: &Parts, name: &str) -> roxmltree::Document<'static> {
        assert!(is_xml_part(name), "{name} 不是 XML 部件");
        let leaked: &'static str = Box::leak(text_of(part(parts, name)).into_boxed_str());
        roxmltree::Document::parse(leaked).unwrap_or_else(|e| panic!("{name} 非良构 XML: {e}"))
    }

    pub fn attr_val(doc: &roxmltree::Document, tag: &str, attr: &str) -> Option<String> {
        doc.descendants()
            .find(|n| n.has_tag_name(tag))
            .and_then(|n| n.attribute(attr).map(|s| s.to_string()))
    }

    /// 不变量 1：每个 XML 部件都良构
    pub fn assert_all_xml_parts_well_formed(parts: &Parts) {
        for (name, _) in parts {
            if is_xml_part(name) {
                parse(parts, name);
            }
        }
    }

    /// 不变量 2：Content_Types 必须覆盖全包，且声明的 Override 都指向真实部件
    pub fn assert_content_types_cover_all_parts(parts: &Parts) {
        let ct = parse(parts, "[Content_Types].xml");
        let declared = |tag: &str, key: &str| -> Vec<String> {
            ct.descendants()
                .filter(|n| n.has_tag_name(tag))
                .filter_map(|n| n.attribute(key).map(|s| s.to_string()))
                .collect()
        };
        let defaults: Vec<String> = declared("Default", "Extension")
            .into_iter()
            .map(|e| e.to_ascii_lowercase())
            .collect();
        let overrides: Vec<String> = declared("Override", "PartName")
            .into_iter()
            .map(|p| p.trim_start_matches('/').to_string())
            .collect();

        for (name, _) in parts {
            let ext = name.rsplit('.').next().unwrap_or("");
            let covered = defaults.iter().any(|d| d == ext) || overrides.iter().any(|o| o == name);
            assert!(covered, "部件 {name} 未在 [Content_Types].xml 里声明");
        }
        for o in &overrides {
            assert!(
                parts.iter().any(|(n, _)| n == o),
                "Override 指向不存在的部件 {o}"
            );
        }
    }

    /// 不变量 3：每条包内关系都要解析到真实部件
    pub fn assert_relationship_targets_resolve(parts: &Parts) {
        for (name, _) in parts.iter().filter(|(n, _)| n.ends_with(".rels")) {
            let doc = parse(parts, name);
            // `_rels/.rels` 的基准是包根，`word/_rels/document.xml.rels` 的基准是 `word/`
            let base = name.rsplit_once("/_rels/").map_or("", |(dir, _)| dir);
            for rel in doc.descendants().filter(|n| n.has_tag_name("Relationship")) {
                if rel.attribute("TargetMode").is_some() {
                    continue; // 外部目标不参与包内解析
                }
                let target = rel.attribute("Target").unwrap_or_default();
                let resolved = if let Some(abs) = target.strip_prefix('/') {
                    abs.to_string()
                } else if base.is_empty() {
                    target.to_string()
                } else {
                    format!("{base}/{target}")
                };
                assert!(
                    parts.iter().any(|(n, _)| *n == resolved),
                    "{name} 的关系目标 {target}（解析为 {resolved}）不在包里"
                );
            }
        }
    }

    /// 三条不变量一次跑完 —— 这就是「Word 打不打得开」的结构判据
    pub fn assert_opc_invariants(parts: &Parts) {
        assert_all_xml_parts_well_formed(parts);
        assert_content_types_cover_all_parts(parts);
        assert_relationship_targets_resolve(parts);
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::export::math::{MathOutcome, omml::to_omml, to_mathml};

    /// 最小可用文档：一段文字 + 真实管线产出的一个 OMML 公式
    fn minimal_package() -> Package {
        Package::new(
            "2026 秋期中测试",
            format!(
                concat!(
                    r#"<w:p><w:pPr><w:pStyle w:val="QuestionNo"/></w:pPr>"#,
                    r#"<w:r><w:t xml:space="preserve">1. 已知 x 满足 </w:t></w:r>"#,
                    "{omml}",
                    r#"<w:r><w:t xml:space="preserve">，求 x。</w:t></w:r></w:p>"#,
                ),
                omml = omml_of(r"\frac{1}{2}")
            ),
        )
    }

    /// 用给定版面打一份最小包，解出 `document.xml`：版面映射只断言真部件里的属性
    fn doc_with_sect(spec: &LayoutSpec) -> roxmltree::Document<'static> {
        let mut pkg = minimal_package();
        pkg.sect_pr = sect_pr(spec, None);
        parse(&unzip(&build(&pkg)), "word/document.xml")
    }

    fn omml_of(latex: &str) -> String {
        let MathOutcome::Ok(mathml) = to_mathml(latex, false) else {
            panic!("用例公式必须能转换: {latex}");
        };
        match to_omml(&mathml) {
            MathOutcome::Ok(omml) => omml,
            MathOutcome::Failed(why) => panic!("OMML 转换失败: {why}"),
        }
    }

    #[test]
    fn package_has_the_required_parts() {
        let parts = unzip(&build(&minimal_package()));
        let names: Vec<&str> = parts.iter().map(|(n, _)| n.as_str()).collect();
        for want in [
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/_rels/document.xml.rels",
            "word/styles.xml",
            "word/settings.xml",
            "docProps/core.xml",
            "docProps/app.xml",
        ] {
            assert!(names.contains(&want), "缺部件 {want}，实际 {names:?}");
        }
        assert_eq!(names[0], "[Content_Types].xml", "入口部件排在最前");
    }

    /// 三条结构不变量：XML 良构 + Content_Types 覆盖全包 + 关系目标真实存在
    #[test]
    fn structure_invariants_hold() {
        let parts = unzip(&build(&minimal_package()));
        assert_opc_invariants(&parts);
    }

    #[test]
    fn document_carries_text_and_editable_math() {
        let parts = unzip(&build(&minimal_package()));
        let doc = parse(&parts, "word/document.xml");
        let root = doc.root_element();
        for ns in [NS_W, NS_M, NS_R, NS_WP, NS_A, NS_PIC] {
            assert!(
                root.namespaces().any(|n| n.uri() == ns),
                "document.xml 根元素缺少命名空间 {ns}"
            );
        }
        assert_eq!(
            doc.descendants()
                .filter(|n| n.has_tag_name((NS_M, "oMath")))
                .count(),
            1,
            "一个公式应有一个 m:oMath"
        );
        let math_text: Vec<&str> = doc
            .descendants()
            .filter(|n| n.has_tag_name((NS_M, "t")))
            .map(|n| n.text().unwrap_or_default())
            .collect();
        assert_eq!(math_text, vec!["1", "2"], "OMML 内容应来自真实转换管线");
        let w_text: Vec<&str> = doc
            .descendants()
            .filter(|n| n.has_tag_name((NS_W, "t")))
            .map(|n| n.text().unwrap_or_default())
            .collect();
        assert_eq!(w_text, vec!["1. 已知 x 满足 ", "，求 x。"]);
        assert!(
            doc.descendants().any(|n| n.has_tag_name((NS_W, "sectPr"))),
            "body 末尾必须有 sectPr"
        );
    }

    #[test]
    fn styles_and_settings_have_the_prescribed_defaults() {
        let parts = unzip(&build(&minimal_package()));
        let styles = parse(&parts, "word/styles.xml");
        let normal = styles
            .descendants()
            .find(|n| n.has_tag_name("style") && n.attribute("styleId") == Some("Normal"))
            .expect("Normal 样式");
        assert_eq!(
            normal.attribute("default"),
            Some("1"),
            "Normal 必须是默认样式"
        );
        assert_eq!(
            attr_val(&styles, "rFonts", "eastAsia").as_deref(),
            Some("宋体")
        );
        assert_eq!(
            attr_val(&styles, "rFonts", "ascii").as_deref(),
            Some("Times New Roman")
        );
        assert_eq!(
            attr_val(&styles, "sz", "val").as_deref(),
            Some("21"),
            "10.5pt = 21 半磅"
        );
        for id in ["SectionTitle", "QuestionNo", "Choice", "Callout"] {
            assert!(
                styles
                    .descendants()
                    .any(|n| n.has_tag_name("style") && n.attribute("styleId") == Some(id)),
                "缺样式 {id}"
            );
        }
        // 样式引用的 pStyle 必须真实存在（写错 id 的表现是格式静默失效）
        let doc = parse(&parts, "word/document.xml");
        for used in doc.descendants().filter(|n| n.has_tag_name("pStyle")) {
            let id = used.attribute("val").unwrap_or_default();
            assert!(
                styles
                    .descendants()
                    .any(|n| n.has_tag_name("style") && n.attribute("styleId") == Some(id)),
                "document.xml 用了未定义样式 {id}"
            );
        }

        let settings = parse(&parts, "word/settings.xml");
        assert_eq!(
            attr_val(&settings, "mathFont", "val").as_deref(),
            Some("Cambria Math"),
            "settings.xml 必须带 m:mathPr/m:mathFont"
        );
    }

    #[test]
    fn media_parts_and_rels_travel_together() {
        // 字节完整性测试不需要真 PNG，docx 打包阶段不看内容（格式嗅探在 assets.rs）
        let png: Vec<u8> = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0xff];
        let bytes = build(&Package {
            title: "含图卷".into(),
            body: concat!(
                r#"<w:p><w:r><w:drawing><wp:inline>"#,
                r#"<wp:extent cx="914400" cy="914400"/><wp:docPr id="1" name="图1"/>"#,
                r#"<a:graphic><a:graphicData><pic:pic>"#,
                r#"<pic:blipFill><a:blip r:embed="rId100"/></pic:blipFill>"#,
                r#"</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#,
            )
            .to_string(),
            sect_pr: sect_pr(&LayoutSpec::default(), None),
            extra_parts: vec![],
            extra_rels: vec![ExtraRel {
                id: "rId100".into(),
                kind: "image".into(),
                target: "media/image1.png".into(),
            }],
            media: vec![("media/image1.png".into(), png.clone())],
        });
        let parts = unzip(&bytes);
        assert_eq!(
            part(&parts, "word/media/image1.png"),
            &png[..],
            "图片字节不得被改写"
        );
        // 媒体包同样要过三条不变量 —— 这才是「Word 能不能打开」的判据
        assert_opc_invariants(&parts);
        let doc = parse(&parts, "word/document.xml");
        assert!(doc.descendants().any(|n| n.has_tag_name((NS_A, "blip"))));
    }

    #[test]
    fn title_is_escaped_in_docprops() {
        let pkg = Package {
            title: r#"A & B <x> "引号""#.into(),
            ..minimal_package()
        };
        let parts = unzip(&build(&pkg));
        let raw = text_of(part(&parts, "docProps/core.xml"));
        assert!(raw.contains(r#"A &amp; B &lt;x&gt;"#), "{raw}");
        assert!(!raw.contains("<x>"), "原始尖括号不得进部件: {raw}");
        assert!(!raw.contains(r#""引号""#), "裸引号不得进部件: {raw}");
        parse(&parts, "docProps/core.xml");
    }

    #[test]
    fn page_setup_is_a4_portrait() {
        let parts = unzip(&build(&minimal_package()));
        let doc = parse(&parts, "word/document.xml");
        assert_eq!(attr_val(&doc, "pgSz", "w").as_deref(), Some("11906"));
        assert_eq!(attr_val(&doc, "pgSz", "h").as_deref(), Some("16838"));
        // 纵向纸不许带 w:orient：Word 只在宽>高时才靠它转方向
        assert_eq!(attr_val(&doc, "pgSz", "orient").as_deref(), None);
        // 边距取自 LayoutSpec::default()（上下 22mm、左右 18mm），不再是从前那套四边 2.5cm
        assert_eq!(attr_val(&doc, "pgMar", "top").as_deref(), Some("1247"));
        assert_eq!(attr_val(&doc, "pgMar", "left").as_deref(), Some("1020"));
        assert_eq!(attr_val(&doc, "pgMar", "gutter").as_deref(), Some("0"));
        assert_eq!(attr_val(&doc, "cols", "num").as_deref(), Some("1"));
    }

    /// 版面 → `sectPr` 的映射（T4.12）：两枚 A3 预设各代表一种 docx 从前根本没有的形状
    #[test]
    fn sect_pr_follows_the_layout_spec() {
        let fold = doc_with_sect(&LayoutSpec::preset("a3_fold_exam").unwrap());
        // 420×297：宽>高必须显式 landscape，Word 不会自己把页面转过来
        assert_eq!(attr_val(&fold, "pgSz", "w").as_deref(), Some("23811"));
        assert_eq!(attr_val(&fold, "pgSz", "h").as_deref(), Some("16838"));
        assert_eq!(
            attr_val(&fold, "pgSz", "orient").as_deref(),
            Some("landscape")
        );
        assert_eq!(attr_val(&fold, "cols", "num").as_deref(), Some("2"));
        // CenterFold 那 20mm 是**栏间**折痕，w:gutter 只认装订侧 ⇒ 折痕退成栏距、gutter 归 0
        assert_eq!(attr_val(&fold, "pgMar", "gutter").as_deref(), Some("0"));
        assert_eq!(attr_val(&fold, "cols", "space").as_deref(), Some("1134"));
        // pgMar 的左值始终是**声明**边距：生效左边距 = left + gutter，装订带不许在两边各扣一次
        assert_eq!(attr_val(&fold, "pgMar", "left").as_deref(), Some("907"));

        let tri = doc_with_sect(&LayoutSpec::preset("a3_tri_exam").unwrap());
        assert_eq!(attr_val(&tri, "cols", "num").as_deref(), Some("3"));
        assert_eq!(attr_val(&tri, "cols", "space").as_deref(), Some("567"));
        // Left 装订带映射成 w:gutter：14mm 边距 + 20mm 带 = PDF 侧那个 34mm 生效左边距
        assert_eq!(attr_val(&tri, "pgMar", "left").as_deref(), Some("794"));
        assert_eq!(attr_val(&tri, "pgMar", "gutter").as_deref(), Some("1134"));
    }

    /// 调用方追加的 XML 部件（页脚）：Override、关系、sectPr 引用三者必须齐备，
    /// 且 `footerReference` 排在 `pgSz` 之前 —— 真实的页脚内容由 writer.rs（T2.7）产出。
    #[test]
    fn extra_part_is_declared_referenced_and_ordered_first() {
        let footer = concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#,
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">"#,
            "<w:p><w:r><w:t>1</w:t></w:r></w:p></w:ftr>",
        );
        let mut pkg = minimal_package();
        pkg.sect_pr = sect_pr(&LayoutSpec::default(), Some("rId3"));
        pkg.extra_parts.push(ExtraPart {
            name: "word/footer1.xml".into(),
            content_type: CT_FOOTER.into(),
            xml: footer.into(),
        });
        pkg.extra_rels.push(ExtraRel {
            id: "rId3".into(),
            kind: "footer".into(),
            target: "footer1.xml".into(),
        });

        let parts = unzip(&build(&pkg));
        assert_opc_invariants(&parts);
        assert!(
            text_of(part(&parts, "[Content_Types].xml"))
                .contains(r#"PartName="/word/footer1.xml""#),
            "页脚部件必须有 Override"
        );

        let doc = parse(&parts, "word/document.xml");
        let sect = doc
            .descendants()
            .find(|n| n.has_tag_name((NS_W, "sectPr")))
            .expect("sectPr");
        let kids: Vec<&str> = sect
            .children()
            .filter(|n| n.is_element())
            .map(|n| n.tag_name().name())
            .collect();
        let footer_at = kids
            .iter()
            .position(|t| *t == "footerReference")
            .unwrap_or_else(|| panic!("缺 footerReference：{kids:?}"));
        let pgsz_at = kids
            .iter()
            .position(|t| *t == "pgSz")
            .unwrap_or_else(|| panic!("缺 pgSz：{kids:?}"));
        assert!(
            footer_at < pgsz_at,
            "footerReference 必须排在 pgSz 之前：{kids:?}"
        );
        assert_eq!(
            attr_val(&doc, "footerReference", "id").as_deref(),
            Some("rId3")
        );
    }

    /// DoD 探针：把最小 docx 落到磁盘，交给 `scripts/check_docx_opens.ps1` 用真 Word / WPS 打开。
    ///
    /// 标 `#[ignore]` 是因为这条判据依赖本机装了 Office —— CI 上没有，跑它只会假失败。
    /// 验收时显式执行：`cargo test --lib export::docx -- --ignored`
    #[test]
    #[ignore = "需要本机 Word / WPS，验收时显式跑"]
    fn writes_probe_docx_for_word_and_wps() {
        let path =
            std::env::var("DOCX_PROBE_PATH").unwrap_or_else(|_| "target/t26_probe.docx".into());
        if let Some(parent) = std::path::Path::new(&path).parent() {
            std::fs::create_dir_all(parent).expect("探针目录可创建");
        }
        let bytes = build(&minimal_package());
        std::fs::write(&path, &bytes).expect("探针 docx 可写入");
        println!("probe_docx={path} bytes={}", bytes.len());
        assert!(
            unzip(&bytes).iter().any(|(n, _)| n == "word/document.xml"),
            "落盘字节必须就是刚才验过的那份包"
        );
    }
}
