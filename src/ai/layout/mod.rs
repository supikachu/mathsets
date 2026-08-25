//! OCR 版面块：保留页码 / 阅读序 / 坐标 / 类型，供规则切题。
//!
//! MinerU `content_list.json` 是主来源；Doc2X 用分页 Markdown 伪块；
//! 无版面时 `source=markdown`，切题仍回退字符串题号。

mod mineru;
mod split;

pub use mineru::parse_mineru_content_list;
pub use split::split_question_chunks;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// 版面来源。`markdown` / `none` 表示没有真实 bbox，切题可能回退。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutSource {
    Mineru,
    Doc2xPages,
    Markdown,
    None,
}

impl LayoutSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mineru => "mineru",
            Self::Doc2xPages => "doc2x_pages",
            Self::Markdown => "markdown",
            Self::None => "none",
        }
    }

    pub fn has_bbox(self) -> bool {
        matches!(self, Self::Mineru)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    Title,
    Text,
    Formula,
    Image,
    Table,
    HeaderFooter,
    Section,
}

impl BlockKind {
    pub fn is_noise(self) -> bool {
        matches!(self, Self::HeaderFooter)
    }
}

/// 归一化坐标，约 0–1000（MinerU content_list）；未知为 None。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

impl BBox {
    pub fn from_xyxy(v: &[f64]) -> Option<Self> {
        if v.len() < 4 {
            return None;
        }
        Some(Self {
            x0: v[0],
            y0: v[1],
            x1: v[2],
            y1: v[3],
        })
    }

    pub fn x_center(self) -> f64 {
        (self.x0 + self.x1) / 2.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutBlock {
    pub page: u32,
    pub order: u32,
    pub kind: BlockKind,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bbox: Option<BBox>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutDocument {
    pub source: LayoutSource,
    pub blocks: Vec<LayoutBlock>,
}

impl LayoutDocument {
    pub fn empty(source: LayoutSource) -> Self {
        Self {
            source,
            blocks: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.iter().all(|b| b.text.trim().is_empty() && b.image_url.is_none())
    }

    /// Doc2X 分页标记；拼进 Markdown 以便无 bbox 时仍能分页。
    pub const PAGE_BREAK: &'static str = "<!--ms-page-->";

    /// 从拼接 Markdown 构造伪块（无 bbox）。
    pub fn from_markdown(md: &str, source: LayoutSource) -> Self {
        let pages: Vec<&str> = if md.contains(Self::PAGE_BREAK) {
            md.split(Self::PAGE_BREAK).collect()
        } else {
            vec![md]
        };
        let mut blocks = Vec::new();
        let mut order = 0u32;
        for (pi, page) in pages.iter().enumerate() {
            for para in page.split("\n\n") {
                let text = para.trim();
                if text.is_empty() || text == Self::PAGE_BREAK {
                    continue;
                }
                blocks.push(LayoutBlock {
                    page: pi as u32,
                    order,
                    kind: classify_markdown_para(text),
                    text: text.to_string(),
                    bbox: None,
                    image_url: first_markdown_image(text),
                });
                order += 1;
            }
        }
        let source = if pages.len() > 1 && source == LayoutSource::Markdown {
            LayoutSource::Doc2xPages
        } else {
            source
        };
        Self { source, blocks }
    }
}

pub fn layout_sidecar_path(upload_dir: &str, task_id: Uuid) -> PathBuf {
    Path::new(upload_dir)
        .join("ocr")
        .join(task_id.to_string())
        .join("layout.json")
}

pub fn load_layout_sidecar(upload_dir: &str, task_id: Uuid) -> Option<LayoutDocument> {
    let path = layout_sidecar_path(upload_dir, task_id);
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn classify_markdown_para(text: &str) -> BlockKind {
    let t = text.trim();
    if t.contains("注意事项") || t.contains("注意事項") {
        return BlockKind::Section;
    }
    if exam_section_heading(t) {
        return BlockKind::Section;
    }
    if t.starts_with("![") {
        return BlockKind::Image;
    }
    if t.starts_with('|') && t.contains('|') {
        return BlockKind::Table;
    }
    BlockKind::Text
}

fn first_markdown_image(text: &str) -> Option<String> {
    let start = text.find("](")?;
    let rest = &text[start + 2..];
    let end = rest.find(')')?;
    let url = rest[..end].trim();
    if url.is_empty() {
        None
    } else {
        Some(url.to_string())
    }
}

pub(crate) fn exam_section_heading(line: &str) -> bool {
    section_heading_line_regex().is_match(normalize_section_line(line))
}

fn normalize_section_line(line: &str) -> &str {
    line.trim().trim_start_matches('#').trim()
}

fn section_heading_line_regex() -> &'static regex::Regex {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"^(?:第\s*[IⅠⅡⅢIV一二三四五1-9]+\s*卷)|^[一二三四五六七八九十]+[、．，.､]?\s*(?:选择|多选|填空|解答|计算|证明|综合)",
        )
        .expect("section heading line")
    });
    &RE
}

fn section_heading_search_regex() -> &'static regex::Regex {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"(?m)(?:^|\n)\s*#*\s*(?:第\s*[IⅠⅡⅢIV一二三四五1-9]+\s*卷|[一二三四五六七八九十]+[、．，.､]?\s*(?:选择|多选|填空|解答|计算|证明|综合))",
        )
        .expect("section heading search")
    });
    &RE
}

/// 正文之后的「二、选择题 / 三、填空题」卷头，应交给下一题而不是留在本题解析末尾。
pub(crate) fn split_trailing_exam_section(text: &str) -> (&str, &str) {
    let mut last: Option<usize> = None;
    for m in section_heading_search_regex().find_iter(text) {
        let mut start = m.start();
        if text.as_bytes().get(start) == Some(&b'\n') {
            start += 1;
        }
        if text[..start].trim().is_empty() {
            continue;
        }
        last = Some(m.start());
    }
    match last {
        Some(i) => (text[..i].trim_end(), text[i..].trim()),
        None => (text, ""),
    }
}

/// 把误粘在上一题末尾的大题说明挪到下一题开头。
pub(crate) fn rehome_trailing_exam_sections(chunks: Vec<String>) -> Vec<String> {
    let mut carry = String::new();
    let mut out: Vec<String> = Vec::new();
    for chunk in chunks {
        let mut md = if carry.is_empty() {
            chunk
        } else {
            format!("{carry}\n\n{chunk}")
        };
        carry.clear();
        let (body, trail) = split_trailing_exam_section(&md);
        if !trail.is_empty() {
            carry = trail.to_string();
            md = body.to_string();
        }
        if !md.trim().is_empty() {
            out.push(md);
        }
    }
    out
}

/// 去掉题干/解析里误入的大题说明（含 Markdown `## 二、选择题`）。
pub(crate) fn strip_exam_sections(text: &str) -> String {
    let (body, _) = split_trailing_exam_section(text);
    let t = body.trim();
    if exam_section_heading(t.lines().next().unwrap_or("")) {
        let rest = t
            .find('\n')
            .map(|i| t[i + 1..].trim_start())
            .unwrap_or("");
        return rest.to_string();
    }
    t.to_string()
}

pub(crate) fn question_start_regex() -> &'static regex::Regex {
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(
            r"^\s*(?:\*\*|__|#+\s*)?(?:第\s*)?([1-9]\d{0,2})\s*(?:题|[.．、]\s|[.．、][\u{4e00}-\u{9fff}]|[.．、]$)",
        )
        .expect("question start")
    });
    &RE
}

pub(crate) fn question_major_no(line: &str) -> Option<u32> {
    question_start_regex()
        .captures(line.trim())
        .and_then(|c| c.get(1))
        .and_then(|m| m.as_str().parse().ok())
}

/// OCR 常把「（2）若过…」收成行首「2. 若过…」，题号从 16 掉到 2，应视为小问而非新大题。
pub(crate) fn is_implausible_major_no_drop(prev: u32, curr: u32) -> bool {
    curr < prev && curr <= 9 && prev >= 10
}

pub(crate) fn is_instruction_numbered_line(line: &str) -> bool {
    const HINTS: &[&str] = &[
        "答卷前",
        "考生务必",
        "准考证",
        "答题卡",
        "用铅笔",
        "用橡皮",
        "本试卷",
        "写在本试卷",
        "考试结束",
        "一并交回",
        "密封线",
        "填涂",
        "选出每小题",
        "回答选择题时",
        "注意事项",
    ];
    HINTS.iter().any(|h| line.contains(h))
}

pub(crate) fn looks_like_math_question_start(line: &str) -> bool {
    const MATH: &[&str] = &[
        "已知", "设", "若", "如图", "函数", "求证", "计算", "下列", "椭圆", "集合", "向量",
        "不等式", "证明：", "证明:",
    ];
    MATH.iter().any(|h| line.contains(h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_markdown_splits_doc2x_pages() {
        let md = "页一第一段\n\n<!--ms-page-->\n\n页二第一段";
        let doc = LayoutDocument::from_markdown(md, LayoutSource::Markdown);
        assert_eq!(doc.source, LayoutSource::Doc2xPages);
        assert_eq!(doc.blocks.len(), 2);
        assert_eq!(doc.blocks[0].page, 0);
        assert_eq!(doc.blocks[1].page, 1);
    }

    #[test]
    fn exam_section_heading_strips_markdown_hashes() {
        assert!(exam_section_heading("## 二、选择题：本题共3小题，每小题6分"));
        assert!(exam_section_heading("三、填空题：本题共3小题，每小题5分，共15分。"));
        assert!(!exam_section_heading("本题考查解析几何中的椭圆"));
        assert!(strip_exam_sections("故选：B\n\n## 三、填空题：本题共3小题，每小题5分，共15分。").contains("故选：B"));
        assert!(!strip_exam_sections("故选：B\n\n## 三、填空题：本题共3小题，每小题5分，共15分。").contains("填空题"));
    }

    #[test]
    fn rehome_moves_trailing_section_to_next_chunk() {
        let chunks = rehome_trailing_exam_sections(vec![
            "8. 函数。\n故选：B\n\n## 二、选择题：本题共3小题，每小题6分，共18分。有多项符合题目要求。".into(),
            "9. 正态。".into(),
        ]);
        assert_eq!(chunks.len(), 2);
        assert!(!chunks[0].contains("二、选择题"), "{}", chunks[0]);
        assert!(chunks[1].contains("二、选择题"));
        assert!(chunks[1].contains("9. 正态"));
    }
}
