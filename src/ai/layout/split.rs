//! 版面块 → 大题 Span：去页眉、双栏阅读序、题号切开、跨页并入当前题。

use super::{
    exam_section_heading, is_implausible_major_no_drop, is_instruction_numbered_line,
    looks_like_math_question_start, question_major_no, question_start_regex,
    rehome_trailing_exam_sections, BlockKind, LayoutBlock, LayoutDocument,
};

/// 切出每道大题的 Markdown。切不出至少两道题时返回 None，调用方回退字符串切块。
pub fn split_question_chunks(doc: &LayoutDocument) -> Option<Vec<String>> {
    let ordered = reorder_reading_order(&doc.blocks);
    let spans = cut_spans(&ordered);
    if spans.len() < 2 {
        return None;
    }
    if spans.len() < 2 {
        return None;
    }
    Some(rehome_trailing_exam_sections(
        spans
            .into_iter()
            .flat_map(|s| super::split_markdown_on_question_starts(&s))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
    ))
}

fn reorder_reading_order(blocks: &[LayoutBlock]) -> Vec<LayoutBlock> {
    if blocks.is_empty() {
        return Vec::new();
    }
    let mut by_page: Vec<(u32, Vec<&LayoutBlock>)> = Vec::new();
    for b in blocks {
        match by_page.iter_mut().find(|(p, _)| *p == b.page) {
            Some((_, v)) => v.push(b),
            None => by_page.push((b.page, vec![b])),
        }
    }
    by_page.sort_by_key(|(p, _)| *p);

    let mut out = Vec::with_capacity(blocks.len());
    for (_, page_blocks) in by_page {
        if is_two_column(&page_blocks) {
            // 双栏只重排文字。配图（尤其选择题 2×2 图象选项）若按 x0
            // 分到右栏，会贴到右栏下一题，带图题就会缺图。
            let mut left = Vec::new();
            let mut right = Vec::new();
            let mut figures = Vec::new();
            for b in page_blocks {
                if is_figure(b) {
                    figures.push(b);
                    continue;
                }
                if block_x0(b) < 500.0 {
                    left.push(b);
                } else {
                    right.push(b);
                }
            }
            left.sort_by_key(|b| y_key(b));
            right.sort_by_key(|b| y_key(b));
            let mut page_out: Vec<LayoutBlock> = left
                .into_iter()
                .chain(right)
                .cloned()
                .collect();
            insert_figures(&mut page_out, figures);
            out.extend(page_out);
        } else {
            out.extend(page_blocks.into_iter().cloned());
        }
    }
    for (i, b) in out.iter_mut().enumerate() {
        b.order = i as u32;
    }
    out
}

fn is_figure(b: &LayoutBlock) -> bool {
    b.kind == BlockKind::Image
        || b.image_url.is_some()
        || b.text.trim().starts_with("![")
}

/// 把配图插回「同一页、y 在图上方最近的题干」后面，避免双栏把图抢走。
fn insert_figures(out: &mut Vec<LayoutBlock>, figures: Vec<&LayoutBlock>) {
    for fig in figures {
        let img_y = y0(fig);
        let img_page = fig.page;
        let img_col = column(fig);
        let mut best: Option<(usize, f64, u8)> = None;
        for (i, t) in out.iter().enumerate() {
            if is_figure(t) || t.page != img_page {
                continue;
            }
            let ty = y0(t);
            if ty > img_y + 80.0 {
                continue;
            }
            let col_bonus = u8::from(column(t) == img_col);
            let better = match best {
                None => true,
                Some((_, by, bb)) => ty > by + 1.0 || ((ty - by).abs() <= 1.0 && col_bonus > bb),
            };
            if better {
                best = Some((i, ty, col_bonus));
            }
        }
        match best {
            Some((i, _, _)) => {
                let mut at = i + 1;
                while at < out.len() && is_figure(&out[at]) {
                    at += 1;
                }
                out.insert(at, fig.clone());
            }
            None => out.push(fig.clone()),
        }
    }
}

fn y0(b: &LayoutBlock) -> f64 {
    b.bbox.map(|x| x.y0).unwrap_or(0.0)
}

fn column(b: &LayoutBlock) -> u8 {
    if block_x0(b) >= 500.0 {
        1
    } else {
        0
    }
}

fn block_x0(b: &LayoutBlock) -> f64 {
    b.bbox.map(|x| x.x0).unwrap_or(0.0)
}

fn y_key(b: &LayoutBlock) -> i64 {
    (b.bbox.map(|x| x.y0).unwrap_or(0.0) * 10.0) as i64
}

/// 同时有足够多的左栏块（x0<420）和右栏块（x0>520）视为双栏。
fn is_two_column(page: &[&LayoutBlock]) -> bool {
    let mut left = 0usize;
    let mut right = 0usize;
    let mut with_bbox = 0usize;
    for b in page {
        let Some(bb) = b.bbox else {
            continue;
        };
        if b.kind.is_noise() {
            continue;
        }
        with_bbox += 1;
        if bb.x0 < 420.0 {
            left += 1;
        } else if bb.x0 > 520.0 {
            right += 1;
        }
    }
    with_bbox >= 4 && left >= 2 && right >= 2
}

fn cut_spans(blocks: &[LayoutBlock]) -> Vec<String> {
    let mut starts: Vec<usize> = Vec::new();
    let mut in_notice = false;
    let mut last_major: Option<u32> = None;
    for (i, b) in blocks.iter().enumerate() {
        if b.kind.is_noise() {
            continue;
        }
        let line = first_line(&b.text);
        if line.contains("注意事项") || line.contains("注意事項") {
            in_notice = true;
        }
        if exam_section_heading(&line) || b.kind == BlockKind::Section {
            in_notice = false;
        }
        if is_question_start(b, in_notice) {
            if let (Some(prev), Some(curr)) = (last_major, question_major_no(&line)) {
                if is_implausible_major_no_drop(prev, curr) {
                    continue;
                }
            }
            in_notice = false;
            if let Some(n) = question_major_no(&line) {
                last_major = Some(n);
            }
            starts.push(i);
        }
    }
    if starts.len() < 2 {
        return Vec::new();
    }

    let mut spans = Vec::new();
    for (si, &s) in starts.iter().enumerate() {
        let end = starts.get(si + 1).copied().unwrap_or(blocks.len());
        // 从本题题号起切，不把卷头注意事项并进第 1 题。
        spans.push(render_span(&blocks[s..end]));
    }
    spans
}

fn is_question_start(b: &LayoutBlock, in_notice: bool) -> bool {
    if b.kind.is_noise() {
        return false;
    }
    let line = first_line(&b.text);
    if !question_start_regex().is_match(&line) {
        return false;
    }
    let instruction = is_instruction_numbered_line(&line);
    let math_like = looks_like_math_question_start(&line);
    if instruction || (in_notice && !math_like) {
        return false;
    }
    true
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_string()
}

fn render_span(blocks: &[LayoutBlock]) -> String {
    let mut parts: Vec<String> = Vec::new();
    for b in blocks {
        if b.kind.is_noise() {
            continue;
        }
        let line = first_line(&b.text);
        if is_notice_or_instruction(&line) && !is_figure(b) {
            continue;
        }
        let t = b.text.trim();
        if t.is_empty() {
            if let Some(url) = &b.image_url {
                parts.push(format!("![]({url})"));
            }
            continue;
        }
        parts.push(t.to_string());
    }
    parts.join("\n\n")
}

fn is_notice_or_instruction(line: &str) -> bool {
    line.contains("注意事项")
        || line.contains("注意事項")
        || is_instruction_numbered_line(line)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::layout::{BBox, LayoutSource};

    fn blk(
        page: u32,
        order: u32,
        text: &str,
        x0: f64,
        y0: f64,
        x1: f64,
        y1: f64,
    ) -> LayoutBlock {
        LayoutBlock {
            page,
            order,
            kind: BlockKind::Text,
            text: text.into(),
            bbox: Some(BBox { x0, y0, x1, y1 }),
            image_url: None,
        }
    }

    #[test]
    fn two_column_reads_left_then_right() {
        let doc = LayoutDocument {
            source: LayoutSource::Mineru,
            blocks: vec![
                blk(0, 0, "1. 左栏第一题 已知集合", 80.0, 100.0, 400.0, 160.0),
                blk(0, 1, "13. 右栏第一题 设椭圆", 560.0, 100.0, 920.0, 160.0),
                blk(0, 2, "2. 左栏第二题 已知函数", 80.0, 200.0, 400.0, 260.0),
                blk(0, 3, "14. 右栏第二题 已知向量", 560.0, 200.0, 920.0, 260.0),
            ],
        };
        let chunks = split_question_chunks(&doc).expect("应切出大题");
        assert!(chunks.len() >= 4, "chunks={chunks:?}");
        assert!(chunks[0].contains("1. 左栏第一题"));
        assert!(chunks[1].contains("2. 左栏第二题"));
        assert!(chunks.iter().any(|c| c.contains("13. 右栏第一题")));
        let i2 = chunks.iter().position(|c| c.contains("2. 左栏第二题")).unwrap();
        let i13 = chunks.iter().position(|c| c.contains("13. 右栏第一题")).unwrap();
        assert!(i2 < i13, "左栏应先于右栏");
    }

    #[test]
    fn skips_notice_and_merges_cross_page() {
        let doc = LayoutDocument {
            source: LayoutSource::Mineru,
            blocks: vec![
                LayoutBlock {
                    page: 0,
                    order: 0,
                    kind: BlockKind::HeaderFooter,
                    text: "第 1 页".into(),
                    bbox: Some(BBox {
                        x0: 10.0,
                        y0: 10.0,
                        x1: 900.0,
                        y1: 40.0,
                    }),
                    image_url: None,
                },
                blk(0, 1, "注意事项：", 80.0, 50.0, 900.0, 80.0),
                blk(0, 2, "1.考生务必在答题卡上填涂", 80.0, 90.0, 900.0, 120.0),
                blk(0, 3, "1. 已知函数 $f(x)$ 的值域是", 80.0, 200.0, 900.0, 280.0),
                blk(1, 4, "续：且 $f(1)=0$。", 80.0, 80.0, 900.0, 140.0),
                blk(1, 5, "2. 设椭圆 $C$ 的方程为", 80.0, 200.0, 900.0, 280.0),
            ],
        };
        let chunks = split_question_chunks(&doc).expect("两道大题");
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains("1. 已知函数"));
        assert!(chunks[0].contains("续：且"));
        assert!(!chunks[0].contains("注意事项"));
        assert!(!chunks[0].contains("考生务必"));
        assert!(chunks[1].contains("2. 设椭圆"));
        assert!(!chunks[1].contains("考生务必"));
        assert!(!chunks[0].contains("第 1 页"));
    }

    #[test]
    fn option_graphs_stay_with_question_in_two_column() {
        fn img(page: u32, order: u32, url: &str, x0: f64, y0: f64) -> LayoutBlock {
            LayoutBlock {
                page,
                order,
                kind: BlockKind::Image,
                text: format!("![]({url})"),
                bbox: Some(BBox {
                    x0,
                    y0,
                    x1: x0 + 200.0,
                    y1: y0 + 120.0,
                }),
                image_url: Some(url.into()),
            }
        }
        let doc = LayoutDocument {
            source: LayoutSource::Mineru,
            blocks: vec![
                blk(0, 0, "1. 已知全集 如图阴影", 80.0, 80.0, 400.0, 140.0),
                blk(0, 1, "5. 函数 f(x) 的图象可能是", 80.0, 200.0, 400.0, 260.0),
                img(0, 2, "/uploads/q5a.png", 80.0, 300.0),
                img(0, 3, "/uploads/q5b.png", 560.0, 300.0),
                img(0, 4, "/uploads/q5c.png", 80.0, 440.0),
                img(0, 5, "/uploads/q5d.png", 560.0, 440.0),
                blk(0, 6, "13. 右栏填空 已知向量", 560.0, 80.0, 920.0, 140.0),
            ],
        };
        let chunks = split_question_chunks(&doc).expect("应切出大题");
        let q5 = chunks
            .iter()
            .find(|c| c.contains("5. 函数"))
            .expect("第5题");
        assert!(q5.contains("/uploads/q5a.png"), "A 图应留在第5题: {q5}");
        assert!(q5.contains("/uploads/q5b.png"), "B 图不应被双栏抢走: {q5}");
        assert!(q5.contains("/uploads/q5c.png"), "{q5}");
        assert!(q5.contains("/uploads/q5d.png"), "{q5}");
        let q13 = chunks
            .iter()
            .find(|c| c.contains("13. 右栏"))
            .expect("第13题");
        assert!(
            !q13.contains("/uploads/q5"),
            "右栏题不应吃掉第5题图象: {q13}"
        );
    }

    #[test]
    fn too_few_questions_returns_none() {
        let doc = LayoutDocument {
            source: LayoutSource::Mineru,
            blocks: vec![blk(0, 0, "没有题号的一段说明", 80.0, 80.0, 900.0, 140.0)],
        };
        assert!(split_question_chunks(&doc).is_none());
    }

    #[test]
    fn does_not_split_ocr_subquestion_two_after_item_sixteen() {
        let doc = LayoutDocument {
            source: LayoutSource::Mineru,
            blocks: vec![
                blk(
                    0,
                    0,
                    "16. 已知 A(0,3) 为椭圆上两点.\n（1）求 C 的离心率",
                    80.0,
                    80.0,
                    900.0,
                    160.0,
                ),
                blk(
                    0,
                    1,
                    "2. 若过 P 的直线 l 交 C 于另一点 B，且三角形面积为 9",
                    80.0,
                    180.0,
                    900.0,
                    240.0,
                ),
                blk(0, 2, "法五：当 l 的斜率不存在时", 80.0, 260.0, 900.0, 320.0),
                blk(1, 3, "法六：设线法与法五一致", 80.0, 80.0, 900.0, 140.0),
            ],
        };
        assert!(
            split_question_chunks(&doc).is_none(),
            "16 后出现行首 2. 若过…应视为小问，整份仍是一道大题"
        );
    }

    #[test]
    fn rehomes_exam_section_heading_to_next_question() {
        let doc = LayoutDocument {
            source: LayoutSource::Mineru,
            blocks: vec![
                blk(
                    0,
                    0,
                    "8. 已知函数 $f(x)$。\n故选：B\n\n## 二、选择题：本题共3小题，每小题6分，共18分。在每小题给出的选项中，有多项符合题目要求。",
                    80.0,
                    80.0,
                    900.0,
                    200.0,
                ),
                blk(
                    0,
                    1,
                    "9. 已知随机变量服从正态分布。",
                    80.0,
                    240.0,
                    900.0,
                    300.0,
                ),
            ],
        };
        let chunks = split_question_chunks(&doc).expect("两道大题");
        assert_eq!(chunks.len(), 2, "{chunks:?}");
        assert!(
            !chunks[0].contains("二、选择题") && !chunks[0].contains("多项符合"),
            "卷头不得留在上一题: {}",
            chunks[0]
        );
        assert!(chunks[0].contains("8. 已知函数"));
        assert!(
            chunks[1].contains("二、选择题") && chunks[1].contains("9. 已知随机变量"),
            "卷头应交给下一题: {}",
            chunks[1]
        );
    }

    #[test]
    fn splits_next_major_glued_with_single_newline() {
        let doc = LayoutDocument {
            source: LayoutSource::Markdown,
            blocks: vec![
                blk(
                    0,
                    0,
                    "1. 已知集合 $M=\\{1\\}$ 。故选：A",
                    80.0,
                    80.0,
                    900.0,
                    140.0,
                ),
                blk(
                    0,
                    1,
                    "18. 已知函数 $f(x)=x^{2}+mx+n$ 。\n【答案】略\n【详解】略\n19.已知 $a\\in [0,8]$ ，函数 $f(x)=\\frac{4x-a}{x^2+1}$（1）求 $a$（2）求证不等式。",
                    80.0,
                    160.0,
                    900.0,
                    400.0,
                ),
            ],
        };
        let chunks = split_question_chunks(&doc).expect("应拆出第19题");
        assert!(
            chunks.iter().any(|c| c.contains("19.已知") || c.contains("19. 已知")),
            "漏切第19题: {chunks:?}"
        );
        let i18 = chunks.iter().position(|c| c.contains("18. 已知函数")).expect("18");
        let i19 = chunks
            .iter()
            .position(|c| c.contains("19.已知") || c.contains("19. 已知"))
            .expect("19");
        assert!(i18 < i19);
        assert!(!chunks[i18].contains("19.已知"), "第19题不得留在第18题块里: {}", chunks[i18]);
    }
}
