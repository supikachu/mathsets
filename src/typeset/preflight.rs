//! 印前预检（T5.1，判据口径见实施计划 R12）
//!
//! ## 为什么全部读编译后的帧树
//!
//! 计划原文写的是「收集 typst 编译诊断，逐题定位」。实测两头都不成立（`typst_gen::tests::
//! preflight_probe`）：52 个不可断的拉丁字母塞进 50mm 版心，字一直画到 120.32mm 处，typst
//! 一句诊断都没有 —— 它与 M3 的「溢出可见」是同一套哲学，宁可画到纸外也不报错；而诊断本身
//! 又被 [`compiler::flatten_warnings`] 有意去掉了行列号（源码是我们生成的，行号对教师无意义）。
//! 所以这里的三条判据一律落在**印出来的事实**上：
//!
//! 1. **有效 DPI**：打印宽取自帧树里的 [`PlacedImage::w_mm`]（源码里的 `width:` 说了不算），
//!    像素尺寸由调用方从文件头读来（[`Raster`]）；比值 < [`MIN_PRINT_DPI`] 记警告并建议换 SVG。
//!    SVG 直嵌不进这条 —— 帧树里根本没有它（矢量 `Group`，见 [`compiler::placed_images`] 的说明）。
//! 2. **版面溢流**：任何一段字、一张图的包围盒跨过**纸张边线**（[`compiler::page_sizes`]）即记，
//!    只判纸边不判版心 —— 页眉页脚、装订带、密封线本来就在页边距里画图。
//! 3. **字体回退**：含汉字的段落在帧树里不是由 [`CJK_FAMILIES`] 画的，就是回退字体画的
//!    （豆腐块的可诊断形态，§13.4）。
//!
//! 本模块**只产这三类新问题**。公式降级、图片跳过、缺中文字体那几条在生成期与 `generate_pdf`
//! 里已经产出，由调用方（`export::pdf`）与本模块的结果拼成一份预检清单 —— 同一条故障不该被
//! 两处各报一遍。
//!
//! ## 归属
//!
//! 帧树里没有「第几题」。图片问题靠 [`Raster::question_no`]（`prefetch_assets` 一路持有），
//! 文字溢流只能报到「第几页 + 原文片段」（R12 把「逐题定位」这一条改掉了）。
//!
//! 页码是**类型化输出**：三条判据都填 [`Issue::page`]（1-based 物理页），预览面板按它跳页；
//! `reason` 里那句「第 N 页」给人读，两处必须同源（R14，断言见本模块 tests）。字体回退一族只报
//! 一条，页码取它**首次出现**那页。

use crate::export::model::{Issue, IssueField, IssueSeverity};
use crate::typeset::compiler::{CJK_FAMILIES, PlacedImage, PlacedRun};

/// 打印质量门槛：低于它，位图印出来就能看出马赛克（§6.5）
pub const MIN_PRINT_DPI: f64 = 300.0;

/// 纸边容差（毫米）。贴边本来就印不出来（printer 裁切误差在这一档），而真故障是「出去十几
/// 毫米」量级（实测 P3b：60mm 的纸上字划到 120.32mm），容差不影响判得出。
const EDGE_TOL_MM: f64 = 0.5;

/// 宽高比配对的相对容差：typst 等比缩放，理论上严格相等，留 2% 给毫米读数舍入。
const ASPECT_TOL: f64 = 0.02;

/// 版面上的一张**栅格**图：归属题号 + 内禀像素尺寸（读自文件头，不是显示宽）。
#[derive(Debug, Clone, PartialEq)]
pub struct Raster {
    pub question_no: Option<u32>,
    pub url: String,
    pub px_w: u32,
    pub px_h: u32,
}

/// 一次预检的全部输入：编译回读的帧树事实 + 文档序的栅格清单。
///
/// 前三个切片都按**物理页**下标对齐（[`compiler::placed_pages`] / [`compiler::placed_images`]
/// / [`compiler::page_sizes`] 的口径）。长度不等时按短的处理 —— 调用方拼错了不该 panic，只是
/// 少检几页。
#[derive(Debug)]
pub struct Evidence<'a> {
    pub runs: &'a [Vec<PlacedRun>],
    pub images: &'a [Vec<PlacedImage>],
    pub paper: &'a [(f64, f64)],
    pub rasters: &'a [Raster],
}

/// 跑完三条判据，返回预检**新发现**的问题（与调用方已有的 issues 拼接使用）。
pub fn inspect(ev: &Evidence) -> Vec<Issue> {
    let mut out = Vec::new();
    out.extend(dpi_findings(ev.images, ev.rasters));
    out.extend(overflow_findings(ev.runs, ev.images, ev.paper));
    out.extend(font_fallback_findings(ev.runs));
    out
}

/// 有效 DPI（判据 1）。
///
/// 帧树里的图与 [`Raster`] 按**宽高比**配对、取文档序里最早未占用的一条：typst 等比缩放，
/// 比例是稳定指纹；而文档序本身不可靠 —— `figure_float`（T4.3）会把题干里的图挪进右栏，
/// 它在帧树里就排到了同段后续图之后。配不上就跳过，**绝不猜**：报错题号的预检比没有预检更坏。
pub fn dpi_findings(images: &[Vec<PlacedImage>], rasters: &[Raster]) -> Vec<Issue> {
    let mut used = vec![false; rasters.len()];
    let mut out = Vec::new();
    for (page, prints) in images.iter().enumerate() {
        for im in prints {
            let Some(i) = pick(&used, rasters, im) else {
                continue;
            };
            used[i] = true;
            let r = &rasters[i];
            if im.w_mm <= 0.0 {
                continue;
            }
            let dpi = f64::from(r.px_w) * 25.4 / im.w_mm;
            if dpi >= MIN_PRINT_DPI {
                continue;
            }
            // 一次算好、两处同读：`reason` 里那句「第 N 页」与 `Issue.page` 不许各读各的（R14）
            let page_no = page as u32 + 1;
            out.push(Issue {
                question_no: r.question_no,
                page: Some(page_no),
                field: IssueField::Image,
                severity: IssueSeverity::Warning,
                latex: None,
                reason: format!(
                    "第 {} 页的图 {}（{}×{}px）印成 {:.1}mm 宽，有效 DPI 只有 {:.0}（目标 {:.0}），建议改用 SVG 或换更高清的素材",
                    page_no,
                    r.url,
                    r.px_w,
                    r.px_h,
                    im.w_mm,
                    dpi,
                    MIN_PRINT_DPI
                ),
            });
        }
    }
    out
}

/// 版面溢流（判据 2）：逐页把跨过纸边的内容归并成「一页 × 一个方向」一条 Issue。
pub fn overflow_findings(
    runs: &[Vec<PlacedRun>],
    images: &[Vec<PlacedImage>],
    paper: &[(f64, f64)],
) -> Vec<Issue> {
    let mut out = Vec::new();
    for (page, &(w, h)) in paper.iter().enumerate() {
        let mut hits: Vec<Hit> = Vec::new();
        if let Some(rs) = runs.get(page) {
            for r in rs {
                if r.run.text.is_empty() {
                    continue;
                }
                let what = snippet(&r.run.text);
                push(&mut hits, "右", w - (r.x_mm + r.w_mm), &what);
                push(&mut hits, "左", r.x_mm, &what);
                push(&mut hits, "上", r.y_mm, &what);
                // 文字段没有高度可读（帧树只给基线锚点），锚点掉到纸外才算
                push(&mut hits, "下", h - r.y_mm, &what);
            }
        }
        if let Some(is) = images.get(page) {
            for im in is {
                let what = format!("一张 {:.1}×{:.1}mm 的图", im.w_mm, im.h_mm);
                push(&mut hits, "右", w - (im.x_mm + im.w_mm), &what);
                push(&mut hits, "左", im.x_mm, &what);
                push(&mut hits, "上", im.y_mm, &what);
                push(&mut hits, "下", h - (im.y_mm + im.h_mm), &what);
            }
        }
        out.extend(merge(page, &hits));
    }
    out
}

/// 字体回退（判据 3）：含汉字的段落由非思源画出。同一族名只报一条。
pub fn font_fallback_findings(runs: &[Vec<PlacedRun>]) -> Vec<Issue> {
    let mut seen: Vec<&str> = Vec::new();
    let mut out = Vec::new();
    for (page, rs) in runs.iter().enumerate() {
        for r in rs {
            if !has_han(&r.run.text) || CJK_FAMILIES.contains(&r.run.family.as_str()) {
                continue;
            }
            if seen.contains(&r.run.family.as_str()) {
                continue;
            }
            seen.push(r.run.family.as_str());
            let page_no = page as u32 + 1;
            out.push(Issue {
                question_no: None,
                page: Some(page_no),
                field: IssueField::Other,
                severity: IssueSeverity::Warning,
                latex: None,
                reason: format!(
                    "中文由字体「{}」绘制而非 {}（首次出现于第 {} 页），印出来可能是豆腐块（缺字体或字体回退）",
                    r.run.family,
                    CJK_FAMILIES.join(" / "),
                    page_no
                ),
            });
        }
    }
    out
}

// ---------------------------------------------------------------- 内部件

/// 一处越界读数：方向、超出毫米数、内容摘要。
struct Hit {
    dir: &'static str,
    over_mm: f64,
    what: String,
}

/// `slack` 为「离纸边还剩多少」，负过容差才算越界。
fn push(hits: &mut Vec<Hit>, dir: &'static str, slack: f64, what: &str) {
    if slack < -EDGE_TOL_MM {
        hits.push(Hit {
            dir,
            over_mm: -slack,
            what: what.to_string(),
        });
    }
}

/// 同页同方向并成一条：教师要知道「第几页、往哪边、出去多少、是什么内容」。
fn merge(page: usize, hits: &[Hit]) -> Vec<Issue> {
    let mut out = Vec::new();
    for dir in ["右", "左", "上", "下"] {
        let same: Vec<&Hit> = hits.iter().filter(|h| h.dir == dir).collect();
        let Some(worst) = same.iter().max_by(|a, b| a.over_mm.total_cmp(&b.over_mm)) else {
            continue;
        };
        out.push(Issue {
            question_no: None,
            page: Some(page as u32 + 1),
            field: IssueField::Other,
            severity: IssueSeverity::Warning,
            latex: None,
            reason: format!(
                "第 {} 页有 {} 处内容超出纸张{}边界，最远 {:.1}mm（如：{}）",
                page + 1,
                same.len(),
                dir,
                worst.over_mm,
                worst.what
            ),
        });
    }
    out
}

/// 按宽高比找第一条未被占用的栅格，返回它在 `rasters` 里的下标。
fn pick(used: &[bool], rasters: &[Raster], printed: &PlacedImage) -> Option<usize> {
    if printed.w_mm <= 0.0 || printed.h_mm <= 0.0 {
        return None;
    }
    let got = printed.w_mm / printed.h_mm;
    rasters.iter().enumerate().find_map(|(i, r)| {
        if used[i] || r.px_w == 0 || r.px_h == 0 {
            return None;
        }
        let want = f64::from(r.px_w) / f64::from(r.px_h);
        ((want - got).abs() / want.max(got) < ASPECT_TOL).then_some(i)
    })
}

fn snippet(text: &str) -> String {
    let mut s: String = text.chars().take(16).collect();
    if text.chars().count() > 16 {
        s.push('…');
    }
    s
}

fn has_han(s: &str) -> bool {
    s.chars().any(|c| {
        matches!(
            c,
            '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}'
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typeset::compiler::RenderedRun;

    fn run(text: &str, family: &str, x_mm: f64, y_mm: f64, w_mm: f64) -> PlacedRun {
        PlacedRun {
            x_mm,
            y_mm,
            w_mm,
            run: RenderedRun {
                text: text.to_string(),
                family: family.to_string(),
            },
        }
    }

    fn img(w_mm: f64, h_mm: f64) -> PlacedImage {
        PlacedImage {
            x_mm: 10.0,
            y_mm: 10.0,
            w_mm,
            h_mm,
        }
    }

    fn raster(qno: Option<u32>, url: &str, px_w: u32, px_h: u32) -> Raster {
        Raster {
            question_no: qno,
            url: url.to_string(),
            px_w,
            px_h,
        }
    }

    #[test]
    fn low_dpi_bitmap_warns_with_its_question_and_the_measured_width() {
        // 200×100px 印成 70.56mm = 72dpi（P1 实测的 typst 自然尺寸）
        let prints = vec![vec![img(70.5556, 35.2778)]];
        let rasters = [raster(Some(7), "/uploads/questions/a.png", 200, 100)];
        let issues = dpi_findings(&prints, &rasters);
        assert_eq!(issues.len(), 1, "{issues:?}");
        let i = &issues[0];
        assert_eq!(i.question_no, Some(7), "题号必须跟着图走");
        assert_eq!(i.page, Some(1), "预览面板按页定位（R14）");
        assert_eq!(i.field, IssueField::Image);
        assert_eq!(i.severity, IssueSeverity::Warning);
        let reason = &i.reason;
        assert!(reason.contains("72"), "{reason}");
        assert!(reason.contains("SVG"), "要给出可执行的建议：{reason}");
        assert!(reason.contains("70.6mm"), "报的是印出来的宽度：{reason}");
    }

    #[test]
    fn enough_pixels_stays_silent() {
        // 3000×1500px 印成 86×43mm = 886dpi：高清素材不该被噪声淹没
        let prints = vec![vec![img(86.0, 43.0)]];
        let rasters = [raster(Some(1), "/uploads/questions/big.png", 3000, 1500)];
        assert!(dpi_findings(&prints, &rasters).is_empty());
    }

    #[test]
    fn just_below_the_threshold_still_warns() {
        // 门槛方向要钉住：< 300 就报。299dpi 只差一点，高清素材也不能蒙过去
        let prints = vec![vec![img(254.0, 254.0)]];
        let rasters = [raster(None, "https://cdn/x.png", 2990, 2990)];
        let issues = dpi_findings(&prints, &rasters);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert!(issues[0].reason.contains("299"), "{}", issues[0].reason);
    }

    #[test]
    fn pairing_follows_aspect_ratio_not_document_order() {
        // figure_float 会把题干里的图挪进右栏 → 帧树顺序与 IR 顺序可以相反，靠比例配
        let prints = vec![vec![img(40.0, 20.0), img(30.0, 30.0)]];
        let rasters = [
            raster(Some(1), "square.png", 20, 20),
            raster(Some(2), "wide.png", 60, 30),
        ];
        let issues = dpi_findings(&prints, &rasters);
        assert_eq!(issues.len(), 2, "两张都是低清位图：{issues:?}");
        assert_eq!(issues[0].question_no, Some(2), "帧树里第一枚印出来的是宽图");
        assert_eq!(issues[1].question_no, Some(1), "帧树里第二枚印出来的是方图");
    }

    #[test]
    fn an_unmatchable_print_is_reported_as_nothing() {
        // 比例对不上 = 我方对「这张图对应 IR 里哪一条」没有把握，宁可不报也不猜题号
        let prints = vec![vec![img(40.0, 20.0)]];
        let rasters = [raster(Some(3), "tall.png", 300, 1200)];
        assert!(dpi_findings(&prints, &rasters).is_empty());
    }

    #[test]
    fn overflow_names_the_page_direction_and_how_far() {
        // 实测 P3b 的形状：60mm 纸上「一长串不可断的拉丁字母」划到 120.32mm
        let runs = vec![vec![run(
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
            "Source Han Serif SC",
            5.0,
            9.38,
            115.32,
        )]];
        let issues = overflow_findings(&runs, &[], &[(60.0, 120.0)]);
        assert_eq!(issues.len(), 1, "{issues:?}");
        assert_eq!(issues[0].page, Some(1), "跳页要的是字段不是文案（R14）");
        let reason = &issues[0].reason;
        assert!(reason.contains("第 1 页"), "{reason}");
        assert!(reason.contains("右"), "{reason}");
        assert!(reason.contains("60.3mm"), "超出量 = 120.32 - 60：{reason}");
        assert!(reason.contains("ABCDEFGH"), "要带原文片段：{reason}");
    }

    #[test]
    fn normal_paper_content_is_not_overflow() {
        // 对照组 = 实测 P4：正常折行的中文长段落、页边距里的页脚、栏内的图都不许报
        let runs = vec![vec![
            run(
                "已知函数在区间上单调递增且其图象关于",
                "Source Han Serif SC",
                5.0,
                7.7,
                66.67,
            ),
            run(
                "第 3 页 / 共 8 页",
                "Source Han Serif SC",
                40.0,
                285.0,
                20.0,
            ),
        ]];
        let images = vec![vec![img(30.21, 14.82)]];
        assert!(overflow_findings(&runs, &images, &[(100.0, 300.0)]).is_empty());
    }

    #[test]
    fn touching_the_edge_is_within_tolerance() {
        // 恰好压线（含 0.5mm 容差）不算故障：真故障是「出去十几毫米」量级
        let runs = vec![vec![run("甲", "Source Han Serif SC", 5.0, 7.0, 74.9)]];
        assert!(overflow_findings(&runs, &[], &[(80.0, 200.0)]).is_empty());
    }

    #[test]
    fn one_issue_per_direction_per_page() {
        let runs = vec![vec![
            run(
                "甲甲甲甲甲甲甲甲甲甲甲甲甲甲甲甲甲",
                "Source Han Serif SC",
                5.0,
                7.0,
                90.0,
            ),
            run(
                "乙乙乙乙乙乙乙乙乙乙乙乙乙乙乙乙乙",
                "Source Han Serif SC",
                5.0,
                17.0,
                95.0,
            ),
        ]];
        let issues = overflow_findings(&runs, &[], &[(60.0, 120.0)]);
        assert_eq!(issues.len(), 1, "同页同方向并成一条：{issues:?}");
        assert!(issues[0].reason.contains("2 处"), "{}", issues[0].reason);
        assert!(
            issues[0].reason.contains("40.0mm"),
            "取最远的那处：{}",
            issues[0].reason
        );
    }

    #[test]
    fn han_drawn_by_a_fallback_font_is_a_finding_once() {
        let runs = vec![
            vec![run("集合 A 的子集", "DejaVu Sans", 5.0, 7.0, 20.0)],
            vec![run("另一个豆腐块", "DejaVu Sans", 5.0, 7.0, 20.0)],
            vec![run("这一页是好的", "Source Han Serif SC", 5.0, 7.0, 20.0)],
        ];
        let issues = font_fallback_findings(&runs);
        assert_eq!(issues.len(), 1, "同一族名只报一条：{issues:?}");
        assert_eq!(issues[0].page, Some(1), "{}", issues[0].reason);
        assert!(
            issues[0].reason.contains("DejaVu Sans"),
            "{}",
            issues[0].reason
        );
    }

    #[test]
    fn fallback_reports_where_it_first_appears() {
        let runs = vec![
            vec![run("这页正常", "Source Han Serif SC", 5.0, 7.0, 20.0)],
            vec![run("第 2 页回退", "DejaVu Sans", 5.0, 7.0, 20.0)],
            vec![run("第 3 页也回退", "DejaVu Sans", 5.0, 7.0, 20.0)],
        ];
        let issues = font_fallback_findings(&runs);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].page, Some(2), "跳页要跳到能看见的那一页");
        assert!(
            issues[0].reason.contains("首次出现于第 2 页"),
            "{}",
            issues[0].reason
        );
    }

    #[test]
    fn latin_and_punctuation_runs_are_silent() {
        let runs = vec![vec![
            run("A = {1, 2}", "New Computer Modern Math", 5.0, 7.0, 20.0),
            run("，。", "DejaVu Sans", 5.0, 9.0, 3.0),
        ]];
        assert!(font_fallback_findings(&runs).is_empty());
    }

    /// R14 的契约：`page` 是给机器跳页的，`reason` 里那句「第 N 页」是给人读的，
    /// 两处一旦分叉，教师看到的页码与点过去看到的页码就不是同一张纸。
    #[test]
    fn page_field_and_the_page_named_in_reason_never_disagree() {
        let prints = vec![vec![], vec![img(70.5556, 35.2778)]];
        let rasters = [raster(Some(7), "a.png", 200, 100)];
        let runs = vec![
            vec![run("这页正常", "Source Han Serif SC", 5.0, 7.0, 20.0)],
            vec![run("这页回退", "DejaVu Sans", 5.0, 7.0, 20.0)],
            vec![run(
                "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
                "Source Han Serif SC",
                5.0,
                9.38,
                115.32,
            )],
        ];
        let mut issues = dpi_findings(&prints, &rasters);
        issues.extend(overflow_findings(&runs, &prints, &[(100.0, 300.0); 3]));
        issues.extend(font_fallback_findings(&runs));
        assert_eq!(issues.len(), 3, "三条判据各一条：{issues:?}");
        for i in &issues {
            assert_eq!(i.page, Some(cited_page(&i.reason)), "{}", i.reason);
        }
    }

    /// 从「第 N 页」这类文案里抠出 N —— 只给上面那枚同源断言用，产品代码不许这么读。
    fn cited_page(reason: &str) -> u32 {
        let digits: String = reason
            .split_once("第 ")
            .unwrap_or_else(|| panic!("预检文案必须带页码：{reason}"))
            .1
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        digits
            .parse()
            .unwrap_or_else(|e| panic!("页码读不出来（{reason}）：{e}"))
    }
}
