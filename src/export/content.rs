//! 内容切分器（T1.3）— 实施计划 §5.2
//!
//! 对 stem / analysis / 选项 / 问树文本做一次性扫描，输出 `InlineNode` 序列，
//! Markdown / DOCX / PDF 三种生成器共用。
//!
//! 识别的语法与前端编辑器（`LatexRender.vue` / `QuestionEdit.vue`）对齐：
//! - 数学公式：`$$...$$`、`\[...\]`（块级）；`$...$`、`\(...\)`（行内）；`\$` 转义
//! - 块级单图：`![alt](url){width,align}`（`{}` 配置可选；URL 字段取首个空白分隔
//!   token，兼容 `url =no-invert` 尾部标记）
//! - 并排图组：`:::img-row {align}` 围栏（内部每行一图，非图片行合并为图注）
//! - 管道表格：`| a | b |` + `|---|---|` 分隔行（支持 `:---:` / `---:` 对齐）
//! - 其余文本与 `\n`（LineBreak）
//!
//! 容错原则：定界符未闭合 / 围栏未闭合时按普通文本处理，绝不丢失内容。

use crate::export::model::{ImageAlign, InlineImage, InlineNode, TableAlign};

/// 把一段富文本切分为 InlineNode 序列
pub fn split_content(text: &str) -> Vec<InlineNode> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let len = text.len();
    let mut i = 0usize;
    let mut at_line_start = true;

    while i < len {
        let rest = &text[i..];

        // ── 行首级结构：img-row 围栏 / 管道表格 ──
        if at_line_start {
            if rest.starts_with(":::img-row") {
                if let Some((node, next)) = parse_img_row(text, i) {
                    flush(&mut buf, &mut out);
                    out.push(node);
                    i = next;
                    at_line_start = true;
                    continue;
                }
            }
            if rest.trim_start().starts_with('|') {
                if let Some((node, next)) = parse_table(text, i) {
                    flush(&mut buf, &mut out);
                    out.push(node);
                    i = next;
                    at_line_start = true;
                    continue;
                }
            }
        }

        // ── 行内扫描 ──
        let c = rest.chars().next().unwrap();
        match c {
            '\\' => {
                let after = &rest[1..];
                if after.starts_with('$') {
                    // \$ 转义 → 字面 $
                    buf.push('$');
                    i += 2;
                } else if after.starts_with('[') {
                    // \[...\] 块级公式
                    if let Some(end) = find_from(text, i + 2, "\\]") {
                        flush(&mut buf, &mut out);
                        out.push(InlineNode::Math {
                            latex: text[i + 2..end].trim().to_string(),
                            display: true,
                        });
                        i = end + 2;
                    } else {
                        buf.push('\\');
                        i += 1;
                    }
                } else if after.starts_with('(') {
                    // \(...\) 行内公式
                    if let Some(end) = find_from(text, i + 2, "\\)") {
                        flush(&mut buf, &mut out);
                        out.push(InlineNode::Math {
                            latex: text[i + 2..end].trim().to_string(),
                            display: false,
                        });
                        i = end + 2;
                    } else {
                        buf.push('\\');
                        i += 1;
                    }
                } else {
                    buf.push('\\');
                    i += 1;
                }
            }
            '$' => {
                if rest.starts_with("$$") {
                    // $$...$$ 块级公式
                    if let Some(end) = find_from(text, i + 2, "$$") {
                        flush(&mut buf, &mut out);
                        out.push(InlineNode::Math {
                            latex: text[i + 2..end].trim().to_string(),
                            display: true,
                        });
                        i = end + 2;
                    } else {
                        buf.push_str("$$");
                        i += 2;
                    }
                } else if let Some(end) = find_from(text, i + 1, "$") {
                    // $...$ 行内公式
                    flush(&mut buf, &mut out);
                    out.push(InlineNode::Math {
                        latex: text[i + 1..end].trim().to_string(),
                        display: false,
                    });
                    i = end + 1;
                } else {
                    // 未闭合 → 字面量
                    buf.push('$');
                    i += 1;
                }
            }
            '!' => {
                if rest.starts_with("![" ) {
                    if let Some((alt, url, width, align, consumed)) = parse_image(rest) {
                        flush(&mut buf, &mut out);
                        out.push(InlineNode::Image {
                            alt,
                            url,
                            width,
                            align,
                        });
                        i += consumed;
                    } else {
                        buf.push('!');
                        i += 1;
                    }
                } else {
                    buf.push('!');
                    i += 1;
                }
            }
            '\n' => {
                flush(&mut buf, &mut out);
                out.push(InlineNode::LineBreak);
                i += 1;
                at_line_start = true;
                continue;
            }
            _ => {
                buf.push(c);
                i += c.len_utf8();
            }
        }
        at_line_start = false;
    }

    flush(&mut buf, &mut out);
    out
}

/// 冲刷待输出文本
fn flush(buf: &mut String, out: &mut Vec<InlineNode>) {
    if !buf.is_empty() {
        out.push(InlineNode::Text {
            text: std::mem::take(buf),
        });
    }
}

/// 从 from 起查找子串（返回字节偏移）
fn find_from(text: &str, from: usize, pat: &str) -> Option<usize> {
    text.get(from..)?.find(pat).map(|p| from + p)
}

/// 解析图片 `![alt](url){config}`。
/// 返回 (alt, url, width, align, 消耗的字节数)；语法不完整返回 None。
fn parse_image(s: &str) -> Option<(Option<String>, String, Option<u32>, Option<ImageAlign>, usize)> {
    let rest = s.strip_prefix("![")?;
    let alt_end = rest.find(']')?;
    let alt = &rest[..alt_end];
    let after = rest[alt_end + 1..].strip_prefix('(')?;
    let url_end = after.find(')')?;
    let url_field = &after[..url_end];
    // ![ + alt + ] + ( + url + )
    let mut consumed = 2 + alt_end + 1 + 1 + url_end + 1;
    let tail = &after[url_end + 1..];

    // 可选 {width, align} 配置
    let (width, align) = if tail.starts_with('{') {
        if let Some(cfg_end) = tail.find('}') {
            let cfg = &tail[1..cfg_end];
            consumed += cfg_end + 1;
            parse_image_config(cfg)
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    // URL 取首个空白分隔 token（兼容 "url =no-invert" 尾部标记）
    let url = url_field.split_whitespace().next().unwrap_or("").to_string();
    let alt = if alt.is_empty() {
        None
    } else {
        Some(alt.to_string())
    };
    Some((alt, url, width, align, consumed))
}

/// 解析 `{width:300, align:left}` 配置（对齐前端 parseImageConfig 的宽松匹配）
fn parse_image_config(cfg: &str) -> (Option<u32>, Option<ImageAlign>) {
    let mut width = None;
    let mut align = None;
    for part in cfg.split(',') {
        let part = part.trim();
        if let Some(rest) = strip_prefix_ci(part, "width:") {
            let digits: String = rest.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
            width = digits.parse().ok();
        } else if let Some(rest) = strip_prefix_ci(part, "align:") {
            align = match rest.trim().to_ascii_lowercase().as_str() {
                "left" => Some(ImageAlign::Left),
                "right" => Some(ImageAlign::Right),
                "center" => Some(ImageAlign::Center),
                _ => None,
            };
        }
    }
    (width, align)
}

/// 大小写不敏感的 strip_prefix
fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

/// 解析 `:::img-row {align}` ... `:::` 围栏。
/// start 指向行首的 ":::img-row"；返回 (节点, 围栏结束后偏移)。
fn parse_img_row(text: &str, start: usize) -> Option<(InlineNode, usize)> {
    // 围栏头必须独占一行（可带 {} 配置）
    let nl = text[start..].find('\n').map(|p| start + p)?;
    let header = text[start..nl].trim();
    let cfg = header[":::img-row".len()..].trim();
    let align = if cfg.is_empty() {
        None
    } else if cfg.starts_with('{') && cfg.ends_with('}') {
        parse_image_config(&cfg[1..cfg.len() - 1]).1
    } else {
        // 头部有非法内容 → 不按围栏处理
        return None;
    };

    // 查找闭合 ::: 行
    let mut pos = nl + 1;
    let mut inner_end = None;
    while pos <= text.len() {
        let line_end = text[pos..].find('\n').map(|p| pos + p).unwrap_or(text.len());
        if text[pos..line_end].trim() == ":::" {
            inner_end = Some(pos);
            break;
        }
        if line_end >= text.len() {
            break;
        }
        pos = line_end + 1;
    }
    let close = inner_end?; // 未闭合 → None（按文本处理）
    let inner = &text[nl + 1..close];

    // 逐行解析：整行是一张图 → 图片；非空非图片行 → 图注
    let mut images = Vec::new();
    let mut caption_parts = Vec::new();
    for line in inner.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some((alt, url, width, _row_align_ignored, consumed)) = parse_image(line) {
            if line[consumed..].trim().is_empty() {
                images.push(InlineImage { alt, url, width });
                continue;
            }
        }
        caption_parts.push(line.to_string());
    }

    // 推进到闭合行之后（含其换行符）
    let next = text[close..]
        .find('\n')
        .map(|p| close + p + 1)
        .unwrap_or(text.len());

    Some((
        InlineNode::ImgRow {
            align,
            images,
            caption: if caption_parts.is_empty() {
                None
            } else {
                Some(caption_parts.join(" "))
            },
        },
        next,
    ))
}

/// 解析管道表格。start 指向行首；要求至少两行且第二行为分隔行。
fn parse_table(text: &str, start: usize) -> Option<(InlineNode, usize)> {
    let mut lines = Vec::new();
    let mut pos = start;
    while pos < text.len() {
        let line_end = text[pos..].find('\n').map(|p| pos + p).unwrap_or(text.len());
        let line = text[pos..line_end].trim();
        if line.starts_with('|') {
            lines.push(line);
            pos = if line_end < text.len() {
                line_end + 1
            } else {
                line_end
            };
        } else {
            break;
        }
    }
    if lines.len() < 2 {
        return None;
    }
    let header = split_pipe_line(lines[0]);
    let sep = split_pipe_line(lines[1]);
    if !is_separator_row(&sep) || sep.len() != header.len() {
        return None;
    }
    let aligns: Vec<TableAlign> = sep.iter().map(|c| sep_align(c)).collect();
    let rows: Vec<Vec<String>> = lines[2..].iter().map(|l| split_pipe_line(l)).collect();
    Some((InlineNode::Table { header, aligns, rows }, pos))
}

/// 拆分 `| a | b |` 行为单元格（剥掉首尾管道）
fn split_pipe_line(line: &str) -> Vec<String> {
    let l = line.trim();
    let l = l.strip_prefix('|').unwrap_or(l);
    let l = l.strip_suffix('|').unwrap_or(l);
    l.split('|').map(|c| c.trim().to_string()).collect()
}

/// 分隔行判定：非空且每个单元格形如 `:-:` / `--` / `:--`（至少一个 -）
fn is_separator_row(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells.iter().all(|c| {
            let t = c.trim();
            let core = t.trim_start_matches(':').trim_end_matches(':');
            !core.is_empty() && core.chars().all(|ch| ch == '-')
        })
}

/// 分隔单元格 → 列对齐（`:--:` 居中 / `--:` 右对齐 / 其余左对齐）
fn sep_align(cell: &str) -> TableAlign {
    let t = cell.trim();
    let left = t.starts_with(':');
    let right = t.ends_with(':');
    if left && right {
        TableAlign::Center
    } else if right {
        TableAlign::Right
    } else {
        TableAlign::Left
    }
}

// ═══════════════════════════ 测试（DoD ≥12 例） ═══════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    fn text(t: &str) -> InlineNode {
        InlineNode::Text { text: t.into() }
    }

    fn math(latex: &str, display: bool) -> InlineNode {
        InlineNode::Math {
            latex: latex.into(),
            display,
        }
    }

    fn image(alt: Option<&str>, url: &str, width: Option<u32>, align: Option<ImageAlign>) -> InlineNode {
        InlineNode::Image {
            alt: alt.map(|s| s.to_string()),
            url: url.into(),
            width,
            align,
        }
    }

    /// 1. 纯文本
    #[test]
    fn t01_plain_text() {
        assert_eq!(split_content("解：由题意得"), vec![text("解：由题意得")]);
    }

    /// 2. 换行 → LineBreak，相邻文本合并不拆散
    #[test]
    fn t02_newline_breaks() {
        assert_eq!(
            split_content("第一行\n第二行"),
            vec![text("第一行"), InlineNode::LineBreak, text("第二行")]
        );
        // 连续空行 = 连续 LineBreak（段落分隔）
        assert_eq!(
            split_content("a\n\nb"),
            vec![
                text("a"),
                InlineNode::LineBreak,
                InlineNode::LineBreak,
                text("b")
            ]
        );
    }

    /// 3. 行内 $...$
    #[test]
    fn t03_inline_dollar() {
        assert_eq!(
            split_content("设 $x^2+1$ 为二次式"),
            vec![text("设 "), math("x^2+1", false), text(" 为二次式")]
        );
    }

    /// 4. 行内 \(...\)
    #[test]
    fn t04_inline_paren() {
        assert_eq!(
            split_content("已知 \\(f(x)=x\\)，求导"),
            vec![text("已知 "), math("f(x)=x", false), text("，求导")]
        );
    }

    /// 5. 块级 $$...$$（跨行）
    #[test]
    fn t05_display_dollar() {
        assert_eq!(
            split_content("证明：\n$$\\frac{1}{2}+\\frac{1}{3}$$\n证毕"),
            vec![
                text("证明："),
                InlineNode::LineBreak,
                math("\\frac{1}{2}+\\frac{1}{3}", true),
                InlineNode::LineBreak,
                text("证毕")
            ]
        );
    }

    /// 6. 块级 \[...\]
    #[test]
    fn t06_display_bracket() {
        assert_eq!(
            split_content("计算 \\[\\sqrt{2}\\] 的值"),
            vec![text("计算 "), math("\\sqrt{2}", true), text(" 的值")]
        );
    }

    /// 7. \$ 转义 → 字面 $，不误判为公式
    #[test]
    fn t07_escaped_dollar() {
        assert_eq!(split_content("价格 \\$5 元"), vec![text("价格 $5 元")]);
    }

    /// 8. 裸图片（无配置）
    #[test]
    fn t08_image_plain() {
        assert_eq!(
            split_content("如图：\n![图1](/uploads/questions/a.png)\n如上"),
            vec![
                text("如图："),
                InlineNode::LineBreak,
                image(Some("图1"), "/uploads/questions/a.png", None, None),
                InlineNode::LineBreak,
                text("如上")
            ]
        );
    }

    /// 9. 带 {width, align} 配置的图片
    #[test]
    fn t09_image_with_config() {
        assert_eq!(
            split_content("![图](/uploads/questions/a.png =no-invert){width:300, align:left}"),
            vec![image(
                Some("图"),
                "/uploads/questions/a.png",
                Some(300),
                Some(ImageAlign::Left)
            )]
        );
    }

    /// 10. :::img-row 围栏（含图注 + 容器对齐）
    #[test]
    fn t10_img_row_with_caption() {
        let src = ":::img-row {align:center}\n![](a.png){width:100}\n![](b.png)\n图 1 与图 2\n:::\n后文";
        assert_eq!(
            split_content(src),
            vec![
                InlineNode::ImgRow {
                    align: Some(ImageAlign::Center),
                    images: vec![
                        InlineImage {
                            alt: None,
                            url: "a.png".into(),
                            width: Some(100)
                        },
                        InlineImage {
                            alt: None,
                            url: "b.png".into(),
                            width: None
                        },
                    ],
                    caption: Some("图 1 与图 2".into()),
                },
                text("后文")
            ]
        );
    }

    /// 11. img-row 无头部配置、无图注
    #[test]
    fn t11_img_row_minimal() {
        let src = "前文\n:::img-row\n![](a.png)\n:::";
        assert_eq!(
            split_content(src),
            vec![
                text("前文"),
                InlineNode::LineBreak,
                InlineNode::ImgRow {
                    align: None,
                    images: vec![InlineImage {
                        alt: None,
                        url: "a.png".into(),
                        width: None
                    }],
                    caption: None,
                }
            ]
        );
    }

    /// 12. 管道表格（含对齐行）
    #[test]
    fn t12_pipe_table() {
        let src = "| x | y |\n|:-:|--:|\n| 1 | 2 |\n| 3 | 4 |";
        assert_eq!(
            split_content(src),
            vec![InlineNode::Table {
                header: vec!["x".into(), "y".into()],
                aligns: vec![TableAlign::Center, TableAlign::Right],
                rows: vec![vec!["1".into(), "2".into()], vec!["3".into(), "4".into()]],
            }]
        );
    }

    /// 13. 混合嵌套：文本 + 行内公式 + 换行 + 图片 + 块级公式
    #[test]
    fn t13_mixed_content() {
        let src = "已知 $f(x)=x$，如图：\n![图](a.png)\n则 $$f'(x)=1$$ 成立";
        assert_eq!(
            split_content(src),
            vec![
                text("已知 "),
                math("f(x)=x", false),
                text("，如图："),
                InlineNode::LineBreak,
                image(Some("图"), "a.png", None, None),
                InlineNode::LineBreak,
                text("则 "),
                math("f'(x)=1", true),
                text(" 成立")
            ]
        );
    }

    /// 14. 未闭合 $ → 字面量文本，不丢内容
    #[test]
    fn t14_unclosed_dollar_literal() {
        assert_eq!(split_content("未闭合 $ 公式"), vec![text("未闭合 $ 公式")]);
        assert_eq!(split_content("孤儿 $$"), vec![text("孤儿 $$")]);
    }

    /// 15. 空串 / 纯空白
    #[test]
    fn t15_empty_input() {
        assert_eq!(split_content(""), Vec::<InlineNode>::new());
        assert_eq!(split_content("  "), vec![text("  ")]);
    }

    /// 16. | 开头但第二行不是分隔行 → 不按表格处理（原样文本）
    #[test]
    fn t16_pipe_lines_without_separator() {
        let src = "| a | b |\n| c | d |";
        assert_eq!(
            split_content(src),
            vec![
                text("| a | b |"),
                InlineNode::LineBreak,
                text("| c | d |")
            ]
        );
    }

    /// 17. 未闭合的 :::img-row 围栏 → 按普通文本处理
    #[test]
    fn t17_unclosed_img_row() {
        let src = ":::img-row\n![](a.png)";
        assert_eq!(
            split_content(src),
            vec![
                text(":::img-row"),
                InlineNode::LineBreak,
                image(None, "a.png", None, None)
            ]
        );
    }
}
