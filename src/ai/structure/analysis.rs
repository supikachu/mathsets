use std::sync::LazyLock;

use regex::Regex;

use crate::ai::types::{AnalysisMethod, BlankAnswer, ParsedAnswer, ParsedPart, ParsedQuestion};

/// 不要求行首：题干末尾常写成「…求胜率。【答案】$\frac{1}{2}$」。
static SECTION_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"【[ \t]*(?:小问[ \t]*(?P<sub>[0-9一二三四五六七八九十]+)[ \t]*详解|(?P<ans>答案|解答)|(?P<detail>详解)|(?P<strat>分析)|(?P<an>解析))[ \t]*】|\[\s*(?P<ansb>答案|解答)\s*\]",
    )
    .expect("section head")
});

static TRAILING_HASH_SCORE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\s*##\s*[-+]?\d+(?:\.\d+)?\s*$").expect("hash score")
});

static SOURCE_METHOD_TITLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:方法|法)[ \t]*[一二三四五六七八九十百0-9]+").expect("source method title")
});

static BARE_SECTION_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*(?:(?P<ans>答案)|(?P<an>解析))[ \t]*[:：]")
        .expect("bare section head")
});

/// 解法标题本体（不要求行首）。行内「法五：」也要切开。
static METHOD_TOKEN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"【[ \t]*(?:解法|方法|法)[ \t]*[一二三四五六七八九十百0-9]+[ \t]*】[ \t]*[:：.．、]?|(?:解法|方法|法)[ \t]*[一二三四五六七八九十百0-9]+[ \t]*[:：.．、]?|(?:另解|别解)[ \t]*[:：.．、]?",
    )
    .expect("method token")
});

static SUB_ANSWER_HEAD: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^[ \t]*[（(][ \t]*([0-9一二三四五六七八九十]+)[ \t]*[）)]")
        .expect("sub answer head")
});

/// 题干与其后【答案】/【解析】/【小问详解】的分界。
pub fn split_body_and_tail(md: &str) -> (&str, &str) {
    match find_cut(md) {
        Some(cut) if cut > 0 => (&md[..cut], &md[cut..]),
        Some(0) => ("", md),
        _ => (md, ""),
    }
}

/// Stage2 只送题干，避免把六种解法再挤进 JSON。
pub fn stage2_llm_input(chunk: &str) -> String {
    let (body, tail) = split_body_and_tail(chunk);
    let body = body.trim();
    if body.is_empty() || tail.trim().is_empty() {
        return chunk.to_string();
    }
    // 短【答案】也必须切开；80 字门槛只留给「法一」这类误切风险。
    if has_bracket_section(tail) || tail.chars().count() >= 80 {
        body.to_string()
    } else {
        chunk.to_string()
    }
}

fn has_bracket_section(text: &str) -> bool {
    SECTION_HEAD.is_match(text)
}

/// 法一 / 解法二 / 另解 等标题个数（行首或带冒号的行内标题）。
pub fn count_method_headings(text: &str) -> usize {
    find_method_heads(text).len()
}

pub fn looks_like_analysis_chunk(text: &str) -> bool {
    text.contains("【解析】")
        || text.contains("【分析】")
        || text.contains("【详解】")
        || BARE_SECTION_HEAD.is_match(text)
}

/// 从 OCR 块切出解法数组，写入 `ParsedQuestion.analysis`（不拆 parts）。
pub fn split_chunk_analysis(chunk: &str) -> Vec<AnalysisMethod> {
    let (_, tail) = split_body_and_tail(chunk);
    let tail = tail.trim();
    if tail.is_empty() {
        return Vec::new();
    }
    if find_method_heads(tail).is_empty() {
        if looks_like_analysis_chunk(tail) {
            let content = strip_leading_section_head(tail);
            if content.is_empty() {
                return Vec::new();
            }
            return vec![AnalysisMethod {
                title: "解析".into(),
                content,
            }];
        }
        return Vec::new();
    }
    split_methods(tail)
}

fn strip_leading_section_head(tail: &str) -> String {
    if let Some(m) = SECTION_HEAD.find(tail) {
        let rest = tail[m.end()..].trim();
        if !rest.is_empty() {
            return rest.to_string();
        }
    }
    tail.trim().to_string()
}

pub fn recover_chunk_questions(qs: &mut [ParsedQuestion], source_md: &str) {
    match qs.len() {
        0 => {}
        1 => recover_question_sections(&mut qs[0], source_md),
        _ => {
            for q in qs.iter_mut() {
                let own = q.stem.clone();
                recover_question_sections(q, &own);
            }
        }
    }
}

/// 整份 OCR 原文上再回填一次：合并残片后仍能找回被截断的法五 / 法六。
pub fn recover_parsed_questions(questions: &mut [ParsedQuestion], full_md: &str) {
    if questions.len() == 1 {
        recover_question_sections(&mut questions[0], full_md);
    }
}

pub fn recover_question_sections(q: &mut ParsedQuestion, source_md: &str) {
    let peeled_tail = peel_stem(q);

    let (body, source_tail) = split_body_and_tail(source_md);
    if !body.trim().is_empty()
        && (q.stem.trim().is_empty()
            || q.stem.chars().count() > body.chars().count().saturating_add(40))
    {
        q.stem = body.trim().to_string();
        let _ = peel_stem(q);
    }

    let tail = if !source_tail.trim().is_empty() {
        source_tail.to_string()
    } else {
        peeled_tail.unwrap_or_default()
    };
    if !tail.trim().is_empty() {
        let sections = parse_sections(&tail);
        let script_n = sections.method_count();
        if script_n > 0 || !sections.answers.is_empty() {
            let llm_n = nonempty_method_count(q);
            if script_n > llm_n || llm_n == 0 {
                apply_sections(q, sections);
                q.warnings.retain(|w| {
                    !w.contains("解析为空") && !w.contains("被截断") && !w.contains("请手动补充")
                });
                if q.question_type == "solution" {
                    q.analysis.retain(|a| !a.content.trim().is_empty());
                }
            } else {
                apply_answers_if_empty(q, &sections.answers);
            }
        }
    }
    finalize_parsed_question(q);
}

/// 入库前把题干/选项/答案/卷头说明整理成可预览结构。
pub fn finalize_parsed_question(q: &mut ParsedQuestion) {
    peel_marked_fields(q);
    super::choice::salvage_choice_structure(q);
    super::choice::fill_choice_answers(q);
    super::choice::strip_exam_sections_from_question(q);
    resplit_nested_methods(q);
    prune_strategy_catalog_in_question(q);
    merge_strategy_detail_in_question(q);
    crate::ai::slice::polish_question(q);
    super::choice::fill_choice_answers(q);
}

fn peel_stem(q: &mut ParsedQuestion) -> Option<String> {
    let cut = find_cut(&q.stem)?;
    let tail = q.stem[cut..].to_string();
    if cut == 0 {
        q.stem.clear();
    } else {
        q.stem = q.stem[..cut].trim().to_string();
    }
    Some(tail)
}

/// 模型把【答案】/【详解】留在题干时，入库前再剥一次。
pub fn peel_marked_fields(q: &mut ParsedQuestion) {
    let Some(tail) = peel_stem(q) else {
        return;
    };
    if tail.trim().is_empty() {
        return;
    }
    let sections = parse_sections(&tail);
    apply_answers_if_empty(q, &sections.answers);
    if nonempty_method_count(q) == 0 {
        apply_sections(q, sections);
    }
}

fn find_cut(text: &str) -> Option<usize> {
    let bracket = SECTION_HEAD.find(text).map(|m| m.start());
    let bare = BARE_SECTION_HEAD
        .find(text)
        .map(|m| m.start())
        .filter(|&i| i == 0 || text[..i].ends_with('\n'));
    let method_heads = find_method_heads(text);
    let method = if method_heads.len() >= 2 {
        Some(method_heads[0].start)
    } else {
        None
    };
    [bracket, bare, method].into_iter().flatten().min()
}

#[derive(Default)]
struct Sections {
    answers: Vec<(Option<u32>, String)>,
    general_methods: Vec<AnalysisMethod>,
    part_methods: Vec<(u32, Vec<AnalysisMethod>)>,
}

impl Sections {
    fn method_count(&self) -> usize {
        let parts: usize = self.part_methods.iter().map(|(_, m)| m.len()).sum();
        nonempty_methods(&self.general_methods) + parts
    }

    fn is_empty(&self) -> bool {
        self.answers.iter().all(|(_, t)| t.trim().is_empty())
            && self.method_count() == 0
    }
}

#[derive(Clone, Copy)]
enum HeadKind {
    Answer,
    /// 【分析】：思路摘要，不是独立解法。
    Strategy,
    /// 【详解】：演算步骤。
    Detail,
    /// 【解析】：整段解析；常为空壳，后面紧跟【分析】【详解】。
    Overview,
    SubDetail(u32),
}

fn parse_sections(tail: &str) -> Sections {
    let mut marks: Vec<(usize, usize, HeadKind)> = Vec::new();
    for m in SECTION_HEAD.captures_iter(tail) {
        let full = m.get(0).expect("match");
        let kind = if m.name("ans").is_some() || m.name("ansb").is_some() {
            HeadKind::Answer
        } else if let Some(sub) = m.name("sub") {
            HeadKind::SubDetail(parse_cn_num(sub.as_str()).unwrap_or(1))
        } else if m.name("strat").is_some() {
            HeadKind::Strategy
        } else if m.name("detail").is_some() {
            HeadKind::Detail
        } else {
            HeadKind::Overview
        };
        marks.push((full.start(), full.end(), kind));
    }
    for m in BARE_SECTION_HEAD.captures_iter(tail) {
        let full = m.get(0).expect("match");
        if marks.iter().any(|(s, _, _)| *s == full.start()) {
            continue;
        }
        let kind = if m.name("ans").is_some() {
            HeadKind::Answer
        } else {
            HeadKind::Overview
        };
        marks.push((full.start(), full.end(), kind));
    }
    marks.sort_by_key(|(s, _, _)| *s);

    let mut out = Sections::default();
    if marks.is_empty() {
        out.general_methods = split_methods(tail);
        return out;
    }
    if marks[0].0 > 0 {
        let prefix = tail[..marks[0].0].trim();
        if !prefix.is_empty() {
            out.general_methods.extend(split_methods(prefix));
        }
    }
    let mut pending_strategy = String::new();
    for (i, &(_, end, kind)) in marks.iter().enumerate() {
        let stop = marks.get(i + 1).map(|(s, _, _)| *s).unwrap_or(tail.len());
        let body = tail[end..stop].trim();
        match kind {
            HeadKind::Answer => {
                if body.is_empty() {
                    continue;
                }
                out.answers.extend(split_sub_answers(body));
            }
            HeadKind::Strategy => {
                if body.is_empty() {
                    continue;
                }
                if !pending_strategy.is_empty() {
                    pending_strategy.push_str("\n\n");
                }
                pending_strategy.push_str(body);
            }
            HeadKind::Overview => {
                if body.is_empty() {
                    continue;
                }
                // 【解析】有时直接包着【分析】式提纲（方法一；方法二），不要拆成解法。
                if is_strategy_catalog_blob(body) {
                    if !pending_strategy.is_empty() {
                        pending_strategy.push_str("\n\n");
                    }
                    pending_strategy.push_str(body);
                } else {
                    push_analysis_body(&mut out.general_methods, &mut pending_strategy, body);
                }
            }
            HeadKind::Detail => {
                push_analysis_body(&mut out.general_methods, &mut pending_strategy, body);
            }
            HeadKind::SubDetail(n) => {
                let mut methods = Vec::new();
                push_analysis_body(&mut methods, &mut pending_strategy, body);
                if !methods.is_empty() {
                    out.part_methods.push((n, methods));
                }
            }
        }
    }
    if !pending_strategy.trim().is_empty() {
        out.general_methods.push(AnalysisMethod {
            title: "解析".into(),
            content: pending_strategy.trim().to_string(),
        });
    }
    out
}

fn push_analysis_body(out: &mut Vec<AnalysisMethod>, pending_strategy: &mut String, body: &str) {
    let preamble = std::mem::take(pending_strategy);
    let preamble = preamble.trim();
    let mut methods = split_methods(body);
    if methods.is_empty() {
        if !preamble.is_empty() {
            out.push(AnalysisMethod {
                title: "分析".into(),
                content: preamble.to_string(),
            });
        }
        return;
    }
    // 短【分析】（无方法一提纲）仍并进第一种解法；带「方法一；方法二」的提纲
    // 并进去会被 explode 拆成假解法，详解在时直接丢掉提纲。
    let attach_preamble = !preamble.is_empty()
        && !is_strategy_catalog_blob(preamble)
        && find_method_heads(preamble).is_empty();
    if attach_preamble {
        if methods[0].content.trim().is_empty() {
            methods[0].content = preamble.to_string();
        } else {
            methods[0].content = format!("{preamble}\n\n{}", methods[0].content);
        }
    }
    out.extend(methods);
}

/// 【分析】里用分号列举的「方法一；方法二」是思路提纲，不是【详解】里的独立解法。
fn is_strategy_catalog_blob(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() {
        return false;
    }
    let heads = find_method_heads(t);
    if heads.len() < 2 {
        return false;
    }
    let catalog_hint = t.contains("方法一")
        || t.contains("方法二")
        || t.contains("同法一")
        || t.contains("同法二")
        || t.contains("即可");
    if !catalog_hint {
        return false;
    }
    let mut spans = Vec::new();
    for (i, h) in heads.iter().enumerate() {
        let stop = heads.get(i + 1).map(|n| n.start).unwrap_or(t.len());
        spans.push(&t[h.end..stop]);
    }
    let mathy = spans
        .iter()
        .filter(|s| s.matches('$').count() >= 8)
        .count();
    mathy == 0
}

fn split_methods(text: &str) -> Vec<AnalysisMethod> {
    let heads = find_method_heads(text);
    if heads.is_empty() {
        let t = text.trim();
        if t.is_empty() {
            return Vec::new();
        }
        return vec![AnalysisMethod {
            title: "解析".into(),
            content: t.to_string(),
        }];
    }
    let mut out = Vec::new();
    let prefix = text[..heads[0].start].trim();
    if prefix.chars().count() > 12 {
        out.push(AnalysisMethod {
            title: "解析".into(),
            content: prefix.to_string(),
        });
    }
    for (i, h) in heads.iter().enumerate() {
        let title = text[h.start..h.end]
            .trim()
            .trim_end_matches([':', '：', '.', '．', '、'])
            .trim()
            .to_string();
        let stop = heads.get(i + 1).map(|n| n.start).unwrap_or(text.len());
        let content = text[h.end..stop].trim().to_string();
        if title.is_empty() && content.is_empty() {
            continue;
        }
        out.push(AnalysisMethod { title, content });
    }
    out
}

#[derive(Clone, Copy)]
struct HeadSpan {
    start: usize,
    end: usize,
}

fn find_method_heads(text: &str) -> Vec<HeadSpan> {
    METHOD_TOKEN
        .find_iter(text)
        .filter(|m| is_valid_method_head(text, m.start(), m.as_str()))
        .map(|m| HeadSpan {
            start: m.start(),
            end: m.end(),
        })
        .collect()
}

fn is_valid_method_head(text: &str, start: usize, matched: &str) -> bool {
    if is_at_line_start(text, start) {
        return true;
    }
    has_strong_delim(matched) && is_phrase_boundary(text, start)
}

fn has_strong_delim(matched: &str) -> bool {
    matched.contains('：')
        || matched.contains(':')
        || matched.contains('、')
        || matched.contains('。')
        || matched.contains('．')
        || matched.contains('】')
}

fn is_at_line_start(text: &str, start: usize) -> bool {
    let before = &text[..start];
    if before.is_empty() {
        return true;
    }
    let t = before.trim_end_matches([' ', '\t', '\u{00a0}']);
    t.is_empty()
        || t.ends_with('\n')
        || t.ends_with('\r')
        || t.ends_with('\u{2028}')
        || t.ends_with("<br>")
        || t.ends_with("<br/>")
        || t.ends_with("<br />")
}

fn is_phrase_boundary(text: &str, start: usize) -> bool {
    let before = text[..start].trim_end_matches([' ', '\t', '\u{00a0}']);
    match before.chars().last() {
        None => true,
        Some(c) => matches!(
            c,
            '\n' | '\r'
                | '$'
                | '。'
                | '；'
                | ';'
                | '！'
                | '？'
                | '.'
                | '，'
                | ','
                | ')'
                | '）'
                | ']'
                | '】'
        ),
    }
}

/// 已切好的解法正文里若还嵌着「法五：」，再拆成独立项。
pub fn resplit_nested_methods(q: &mut ParsedQuestion) {
    q.analysis = explode_methods(std::mem::take(&mut q.analysis));
    resplit_part_methods(&mut q.parts);
}

fn resplit_part_methods(parts: &mut [ParsedPart]) {
    for p in parts {
        p.analyses = explode_methods(std::mem::take(&mut p.analyses));
        resplit_part_methods(&mut p.children);
    }
}

fn explode_methods(methods: Vec<AnalysisMethod>) -> Vec<AnalysisMethod> {
    let mut out = Vec::new();
    for m in methods {
        if is_strategy_catalog_blob(&m.content) || find_method_heads(&m.content).is_empty() {
            out.push(m);
            continue;
        }
        let blob = format!("{}：\n{}", m.title.trim(), m.content);
        if is_strategy_catalog_blob(&blob) {
            out.push(m);
            continue;
        }
        let pieces = split_methods(&blob);
        if pieces.len() <= 1 {
            out.push(m);
        } else {
            out.extend(pieces.into_iter().filter(|a| !a.content.trim().is_empty() || !a.title.is_empty()));
        }
    }
    out
}

fn prune_strategy_catalog_in_question(q: &mut ParsedQuestion) {
    q.analysis = prune_catalog_method_items(std::mem::take(&mut q.analysis));
    prune_catalog_in_parts(&mut q.parts);
}

fn prune_catalog_in_parts(parts: &mut [ParsedPart]) {
    for p in parts {
        p.analyses = prune_catalog_method_items(std::mem::take(&mut p.analyses));
        prune_catalog_in_parts(&mut p.children);
    }
}

/// 同一叶子里若已有详解演算，丢掉从【分析】提纲拆出来的短「方法一/法三」。
fn prune_catalog_method_items(methods: Vec<AnalysisMethod>) -> Vec<AnalysisMethod> {
    if methods.len() < 2 {
        return methods;
    }
    let joined = join_method_blob(&methods);
    let all_light_math = methods.iter().all(|m| m.content.matches('$').count() < 8);
    if all_light_math && is_strategy_catalog_blob(&joined) {
        let content = methods
            .iter()
            .map(|m| {
                let body = m.content.trim();
                if m.title == "解析" || m.title == "分析" || m.title.is_empty() {
                    body.to_string()
                } else {
                    format!("{}：{body}", m.title.trim())
                }
            })
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("；");
        return vec![AnalysisMethod {
            title: "分析".into(),
            content,
        }];
    }
    let flags: Vec<bool> = methods.iter().map(is_catalog_method_item).collect();
    let any_real = flags.iter().any(|o| !*o);
    let any_outline = flags.iter().any(|o| *o);
    if any_real && any_outline {
        return methods
            .into_iter()
            .zip(flags)
            .filter(|(_, outline)| !*outline)
            .map(|(m, _)| m)
            .collect();
    }
    methods
}

fn join_method_blob(methods: &[AnalysisMethod]) -> String {
    methods
        .iter()
        .map(|m| format!("{}：{}", m.title, m.content))
        .collect::<Vec<_>>()
        .join("；")
}

fn is_catalog_method_item(m: &AnalysisMethod) -> bool {
    let t = m.content.trim();
    if t.is_empty() {
        return false;
    }
    if t.matches('$').count() >= 8 {
        return false;
    }
    if t.chars().count() > 400 {
        return false;
    }
    m.title.contains("方法")
        || t.contains("同法一")
        || t.contains("同法二")
        || (t.contains("即可") && t.matches('$').count() < 4)
}

/// 【分析】思路摘要 + 【详解】演算是同一种解法，不要变成解法一 / 解法二。
fn merge_strategy_detail_in_question(q: &mut ParsedQuestion) {
    q.analysis = merge_strategy_detail_methods(std::mem::take(&mut q.analysis));
    merge_strategy_detail_in_parts(&mut q.parts);
}

fn merge_strategy_detail_in_parts(parts: &mut [ParsedPart]) {
    for p in parts {
        p.analyses = merge_strategy_detail_methods(std::mem::take(&mut p.analyses));
        merge_strategy_detail_in_parts(&mut p.children);
    }
}

fn merge_strategy_detail_methods(methods: Vec<AnalysisMethod>) -> Vec<AnalysisMethod> {
    if methods.len() < 2 {
        return methods;
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < methods.len() {
        if i + 1 < methods.len() && should_merge_strategy_detail(&methods[i], &methods[i + 1]) {
            let first_title = methods[i].title.clone();
            let mut merged = methods[i + 1].clone();
            let pre = methods[i].content.trim();
            if !pre.is_empty() {
                merged.content = format!("{pre}\n\n{}", merged.content.trim_start());
            }
            if is_llm_ordinal_title(&merged.title) {
                merged.title = if is_llm_ordinal_title(&first_title) {
                    "解析".into()
                } else {
                    first_title
                };
            }
            out.push(merged);
            i += 2;
            continue;
        }
        out.push(methods[i].clone());
        i += 1;
    }
    out
}

fn should_merge_strategy_detail(first: &AnalysisMethod, second: &AnalysisMethod) -> bool {
    // 原文「法一 / 法二」是真·多种解法，不能并。
    if is_source_method_title(&first.title) {
        return false;
    }
    if !is_strategy_blurb(&first.content) {
        return false;
    }
    if !looks_like_worked_solution(&second.content) {
        return false;
    }
    if second.content.trim().chars().count() <= first.content.trim().chars().count() {
        return false;
    }
    true
}

/// 「法一」「方法二」来自解析卷正文；「解法一」多半是模型给【分析】【详解】编的序号。
fn is_source_method_title(title: &str) -> bool {
    let t = title
        .trim()
        .trim_start_matches('【')
        .trim_end_matches('】')
        .trim();
    if t.contains("另解") || t.contains("别解") {
        return true;
    }
    if t.starts_with("解法") {
        return false;
    }
    let stripped = t.trim_start_matches(['(', '（']).trim_end_matches([')', '）']);
    SOURCE_METHOD_TITLE.is_match(stripped.trim())
}

fn is_llm_ordinal_title(title: &str) -> bool {
    let t = title.trim();
    t == "解析"
        || t.starts_with("解法")
        || t == "分析"
        || t == "详解"
        || t.starts_with("【分析】")
        || t.starts_with("【详解】")
}

fn is_strategy_blurb(content: &str) -> bool {
    let t = TRAILING_HASH_SCORE.replace(content.trim(), "").trim().to_string();
    if t.is_empty() {
        return false;
    }
    if t.contains("故选") || t.contains("故填") {
        return false;
    }
    if t.contains("因为") && t.contains("所以") {
        return false;
    }
    if t.chars().count() > 480 {
        return false;
    }
    const HINTS: &[&str] = &[
        "即可得",
        "即可求",
        "即可解",
        "即可判断",
        "即可选",
        "即可求解",
        "即可得解",
        "可求",
        "可判断",
        "可解出",
    ];
    if HINTS.iter().any(|h| t.contains(h)) {
        return true;
    }
    const OPENERS: &[&str] = &[
        "根据",
        "由",
        "设",
        "画出",
        "代入",
        "求出",
        "先求",
        "利用",
        "结合",
        "化简",
        "将",
        "直接",
    ];
    let head = t.trim_start_matches(['（', '(', '1', '2', '3', '4', '5', '6', '7', '8', '9', '0', '一', '二', '三', '）', ')']).trim_start();
    OPENERS.iter().any(|h| head.starts_with(h))
}

fn looks_like_worked_solution(content: &str) -> bool {
    let t = content.trim();
    t.contains("因为")
        || t.contains("所以")
        || t.contains("故选")
        || t.contains("故填")
        || t.contains("依题")
        || t.contains("由题")
        || t.contains("设")
        || t.contains("得")
}

fn split_sub_answers(text: &str) -> Vec<(Option<u32>, String)> {
    let heads: Vec<_> = SUB_ANSWER_HEAD.captures_iter(text).collect();
    if heads.is_empty() {
        let t = text.trim();
        if t.is_empty() {
            return Vec::new();
        }
        return vec![(None, t.to_string())];
    }
    let mut out = Vec::new();
    for (i, cap) in heads.iter().enumerate() {
        let m = cap.get(0).expect("match");
        let n = cap
            .get(1)
            .and_then(|g| parse_cn_num(g.as_str()))
            .unwrap_or(1);
        let start = m.end();
        let stop = heads
            .get(i + 1)
            .and_then(|c| c.get(0))
            .map(|n| n.start())
            .unwrap_or(text.len());
        let content = text[start..stop].trim().to_string();
        if !content.is_empty() {
            out.push((Some(n), content));
        }
    }
    out
}

fn apply_sections(q: &mut ParsedQuestion, sections: Sections) {
    if sections.is_empty() {
        return;
    }
    if q.question_type != "solution" {
        let mut methods = sections.general_methods;
        for (_, m) in sections.part_methods {
            methods.extend(m);
        }
        methods.retain(|a| !a.content.trim().is_empty());
        if !methods.is_empty() {
            q.analysis = methods;
        }
        apply_answers_if_empty(q, &sections.answers);
        return;
    }

    let max_part = sections
        .part_methods
        .iter()
        .map(|(n, _)| *n)
        .max()
        .unwrap_or(0);
    if max_part > 0 {
        ensure_leaf_count(q, max_part);
        for (n, methods) in sections.part_methods {
            if let Some(leaf) = find_leaf_mut(&mut q.parts, n) {
                if !methods.is_empty() {
                    leaf.analyses = methods;
                }
            }
        }
        if !sections.general_methods.is_empty() {
            for leaf in leaves_mut(&mut q.parts) {
                if leaf.analyses.iter().all(|a| a.content.trim().is_empty()) {
                    leaf.analyses = sections.general_methods.clone();
                }
            }
        }
    } else if !sections.general_methods.is_empty() {
        q.ensure_solution_parts();
        if let Some(leaf) = last_leaf_mut(&mut q.parts) {
            leaf.analyses = sections.general_methods;
        } else {
            q.analysis = sections.general_methods;
            q.ensure_solution_parts();
        }
    }
    apply_answers_if_empty(q, &sections.answers);
}

fn apply_answers_if_empty(q: &mut ParsedQuestion, answers: &[(Option<u32>, String)]) {
    if answers.is_empty() {
        return;
    }
    let cleaned: Vec<(Option<u32>, String)> = answers
        .iter()
        .map(|(n, t)| (*n, clean_answer_text(t)))
        .filter(|(_, t)| !t.is_empty())
        .collect();
    if cleaned.is_empty() {
        return;
    }
    if q.question_type == "fill" {
        apply_fill_answers_if_empty(q, &cleaned);
        return;
    }
    if matches!(q.question_type.as_str(), "choice" | "multiple") {
        super::choice::apply_choice_answers_if_empty(q, &cleaned);
        return;
    }
    if q.question_type != "solution" {
        return;
    }
    if q.parts.is_empty() {
        q.ensure_solution_parts();
    }
    for (n, text) in cleaned {
        let leaf = match n {
            Some(i) => find_leaf_mut(&mut q.parts, i),
            None => last_leaf_mut(&mut q.parts),
        };
        if let Some(leaf) = leaf {
            if leaf.answer.as_ref().is_none_or(|a| a.trim().is_empty()) {
                leaf.answer = Some(text);
            }
        }
    }
}

fn apply_fill_answers_if_empty(q: &mut ParsedQuestion, answers: &[(Option<u32>, String)]) {
    let Some(ParsedAnswer::Fill { blanks }) = q.correct_answer.as_mut() else {
        q.correct_answer = Some(ParsedAnswer::Fill {
            blanks: answers
                .iter()
                .enumerate()
                .map(|(i, (_, t))| BlankAnswer {
                    position: (i + 1) as i32,
                    answer: t.clone(),
                })
                .collect(),
        });
        return;
    };
    if blanks.iter().all(|b| b.answer.trim().is_empty()) {
        *blanks = answers
            .iter()
            .enumerate()
            .map(|(i, (_, t))| BlankAnswer {
                position: (i + 1) as i32,
                answer: t.clone(),
            })
            .collect();
    }
}

fn clean_answer_text(s: &str) -> String {
    TRAILING_HASH_SCORE.replace(s.trim(), "").trim().to_string()
}

fn ensure_leaf_count(q: &mut ParsedQuestion, n: u32) {
    q.ensure_solution_parts();
    let simple = q.parts.len() == 1
        && q.parts[0].children.is_empty()
        && q.parts[0].stem.trim().is_empty()
        && n > 1;
    if simple {
        q.parts.clear();
    }
    let mut have = walk_leaf_count(&q.parts);
    while have < n {
        have += 1;
        q.parts.push(empty_leaf(have));
    }
}

fn empty_leaf(n: u32) -> ParsedPart {
    ParsedPart {
        id: uuid::Uuid::new_v4().to_string(),
        label: format!("({n})"),
        stem: String::new(),
        children: Vec::new(),
        answer: Some(String::new()),
        analyses: Vec::new(),
        no_analysis_needed: false,
    }
}

fn walk_leaf_count(parts: &[ParsedPart]) -> u32 {
    parts
        .iter()
        .map(|p| {
            if p.children.is_empty() {
                1
            } else {
                walk_leaf_count(&p.children)
            }
        })
        .sum()
}

fn leaves_mut(parts: &mut [ParsedPart]) -> Vec<&mut ParsedPart> {
    let mut out = Vec::new();
    fn rec<'a>(parts: &'a mut [ParsedPart], out: &mut Vec<&'a mut ParsedPart>) {
        for p in parts {
            if p.children.is_empty() {
                out.push(p);
            } else {
                rec(&mut p.children, out);
            }
        }
    }
    rec(parts, &mut out);
    out
}

fn last_leaf_mut(parts: &mut [ParsedPart]) -> Option<&mut ParsedPart> {
    leaves_mut(parts).into_iter().next_back()
}

fn find_leaf_mut(parts: &mut [ParsedPart], n: u32) -> Option<&mut ParsedPart> {
    let mut numbered: Option<usize> = None;
    {
        let leaves = collect_leaf_ids(parts);
        for (i, (label, _)) in leaves.iter().enumerate() {
            if label_number(label) == Some(n) {
                numbered = Some(i);
                break;
            }
        }
        if numbered.is_none() {
            numbered = n.checked_sub(1).map(|i| i as usize).filter(|&i| i < leaves.len());
        }
    }
    let idx = numbered?;
    leaves_mut(parts).into_iter().nth(idx)
}

fn collect_leaf_ids(parts: &[ParsedPart]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    fn rec(parts: &[ParsedPart], out: &mut Vec<(String, String)>) {
        for p in parts {
            if p.children.is_empty() {
                out.push((p.label.clone(), p.id.clone()));
            } else {
                rec(&p.children, out);
            }
        }
    }
    rec(parts, &mut out);
    out
}

fn label_number(label: &str) -> Option<u32> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[0-9一二三四五六七八九十]+").expect("label num"));
    RE.find(label).and_then(|m| parse_cn_num(m.as_str()))
}

fn nonempty_method_count(q: &ParsedQuestion) -> usize {
    nonempty_methods(&q.analysis) + count_part_methods(&q.parts)
}

fn count_part_methods(parts: &[ParsedPart]) -> usize {
    parts
        .iter()
        .map(|p| nonempty_methods(&p.analyses) + count_part_methods(&p.children))
        .sum()
}

fn nonempty_methods(methods: &[AnalysisMethod]) -> usize {
    methods.iter().filter(|a| !a.content.trim().is_empty()).count()
}

fn parse_cn_num(s: &str) -> Option<u32> {
    let t = s.trim();
    if t.chars().all(|c| c.is_ascii_digit()) {
        return t.parse().ok();
    }
    Some(match t {
        "一" => 1,
        "二" => 2,
        "三" => 3,
        "四" => 4,
        "五" => 5,
        "六" => 6,
        "七" => 7,
        "八" => 8,
        "九" => 9,
        "十" => 10,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse_q(v: serde_json::Value) -> ParsedQuestion {
        serde_json::from_value(v).expect("ParsedQuestion")
    }

    fn lecture_md() -> String {
        "\
16. 已知椭圆 $C:\\dfrac{x^2}{4}+y^2=1$。\n\
（1）求离心率；\n\
（2）若过 $P$ 的直线 $l$ 交 $C$ 于另一点 $B$，求 $l$ 的方程。\n\
【解析】\n\
法一：平移直线\n\
法二：点差法\n\
法三：韦达定理\n\
法四：参数方程\n\
法五：斜率不存在\n\
法六：水平宽乘铅垂高\n\
【小问1详解】\n\
法一：由 $a=2,b=1$ 得 $e=\\dfrac{\\sqrt{3}}{2}$。\n\
法二：定义法求 $e$。\n\
【小问2详解】\n\
法一：平移直线，设斜率。\n\
法二：点差法。\n\
法三：韦达定理。\n\
法四：参数方程。\n\
法五：斜率不存在时。\n\
法六：水平宽乘铅垂高。\n"
            .to_string()
    }

    #[test]
    fn recover_does_not_treat_analysis_method_catalog_as_solutions() {
        let md = "\
16. 已知 $A(0,3)$ 和 $P(3,\\dfrac{3}{2})$ 为椭圆上两点。\n\
（1）求离心率；\n\
（2）求直线 $l$ 的方程。\n\
【答案】（1）$e=\\dfrac{\\sqrt{3}}{2}$（2）$y=x+1$\n\
【解析】\n\
【分析】（1）代入两点得到关于 $a,b$ 的方程，解出即可；\n\
（2）方法一：以 $AP$ 为底，求出三角形的高，即点 $B$ 到直线 $AP$ 的距离，再利用平行线距离公式得到平移后的直线方程，联立椭圆方程得到 $B$ 点坐标，则得到直线 $l$ 的方程；方法二：同法一得到点 $B$ 到直线 $AP$ 的距离后用点差法；法三：同法一得到直线 $AP$ 的方程后联立；法四：参数方程；法五：斜率不存在；法六：水平宽乘铅垂高。\n\
【小问1详解】\n\
法一：由 $a=2,b=1$ 得 $e=\\dfrac{\\sqrt{3}}{2}$。\n\
法二：定义法求 $e$。\n\
【小问2详解】\n\
法一：平移直线，设斜率 $k$，联立椭圆得 $x_1+x_2$。\n\
法二：点差法，由切点弦公式。\n\
法三：韦达定理，列出方程。\n\
法四：参数方程，$x=2\\cos\\theta$。\n\
法五：斜率不存在时，$l:x=3$。\n\
法六：水平宽乘铅垂高，$S=\\dfrac{1}{2}ab$。\n";
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": md,
            "analysis": [{"title": "解法一", "content": ""}],
            "parts": []
        }));
        recover_question_sections(&mut q, md);
        let p1 = q
            .parts
            .iter()
            .find(|p| label_number(&p.label) == Some(1))
            .expect("小问1");
        assert_eq!(p1.analyses.len(), 2, "小问1不应吃进分析提纲: {:?}", p1.analyses.iter().map(|a| &a.title).collect::<Vec<_>>());
        let p2 = q
            .parts
            .iter()
            .find(|p| label_number(&p.label) == Some(2))
            .expect("小问2");
        assert_eq!(p2.analyses.len(), 6, "小问2应是详解六法而非分析提纲: {:?}", p2.analyses.iter().map(|a| &a.title).collect::<Vec<_>>());
        assert!(p2.analyses[0].content.contains("设斜率"), "{}", p2.analyses[0].content);
        assert!(p2.analyses.iter().any(|a| a.content.contains("水平宽")));
        assert!(
            !p2.analyses.iter().any(|a| a.content.contains("以 $AP$ 为底") || a.title.contains("方法")),
            "分析提纲不得成为小问2解法: {:?}",
            p2.analyses.iter().map(|a| (&a.title, a.content.chars().take(40).collect::<String>())).collect::<Vec<_>>()
        );
    }

    #[test]
    fn finalize_collapses_analysis_catalog_tabs() {
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": "16. 椭圆",
            "analysis": [],
            "parts": [{
                "label": "(2)",
                "analyses": [
                    {"title": "方法一", "content": "以 $AP$ 为底，求出三角形的高，再利用平行线距离公式得到直线方程。"},
                    {"title": "方法二", "content": "同法一得到点 $B$ 到直线 $AP$ 的距离后用点差法。"},
                    {"title": "法三", "content": "同法一得到直线 $AP$ 的方程后联立。"},
                    {"title": "法四", "content": "参数方程。"},
                    {"title": "法五", "content": "斜率不存在。"},
                    {"title": "法六", "content": "水平宽乘铅垂高。"}
                ]
            }]
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(q.parts[0].analyses.len(), 1, "分析提纲应合成一条而不是六种解法: {:?}", q.parts[0].analyses.iter().map(|a| &a.title).collect::<Vec<_>>());
        assert!(q.parts[0].analyses[0].content.contains("以 $AP$ 为底"));
        assert!(q.parts[0].analyses[0].content.contains("同法一"));
    }

    #[test]
    fn recover_merges_analysis_and_detail_into_one_method() {
        let md = "\
2. 若 $\\dfrac{z}{z-1}=1+i$，则 $z=$（ ）\n\
A. $1-i$\nB. $-1+i$\nC. $1+i$\nD. $-1-i$\n\
【答案】C\n\
【解析】\n\
【分析】由复数四则运算法则直接运算即可求解。\n\
【详解】因为 $\\dfrac{z}{z-1}=1+i$，所以 $z=1+i$。故选：C。\n";
        let mut q = parse_q(json!({
            "question_type": "choice",
            "stem": "2. 若复数",
            "options": [
                {"label": "A", "content": "1-i"},
                {"label": "B", "content": "-1+i"},
                {"label": "C", "content": "1+i"},
                {"label": "D", "content": "-1-i"}
            ],
            "correct_answer": {"kind": "choice", "value": {"options": []}},
            "analysis": [{"title": "解法一", "content": ""}],
            "parts": []
        }));
        recover_question_sections(&mut q, md);
        assert_eq!(q.analysis.len(), 1, "分析+详解应是一种解法: {:?}", q.analysis.iter().map(|a| &a.title).collect::<Vec<_>>());
        assert!(!q.analysis[0].content.contains("【分析】"), "{}", q.analysis[0].content);
        assert!(!q.analysis[0].content.contains("【详解】"), "{}", q.analysis[0].content);
        assert!(!q.analysis[0].content.contains("四则运算"), "【分析】摘要不应留在解析: {}", q.analysis[0].content);
        assert!(q.analysis[0].content.contains("因为"), "{}", q.analysis[0].content);
        assert!(q.analysis[0].content.contains("故选"), "{}", q.analysis[0].content);
    }

    #[test]
    fn finalize_merges_llm_strategy_and_detail_pair() {
        let mut q = parse_q(json!({
            "question_type": "choice",
            "stem": "已知向量",
            "analysis": [
                {"title": "解法一", "content": "根据向量垂直的坐标运算可求 $x$ 的值。"},
                {"title": "解法二", "content": "因为 $\\vec{b}\\perp(\\vec{b}-4\\vec{a})$，所以 $\\vec{b}\\cdot(\\vec{b}-4\\vec{a})=0$，故 $x=2$。故选：D。"}
            ],
            "parts": []
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(q.analysis.len(), 1, "{:?}", q.analysis.iter().map(|a| a.content.chars().count()).collect::<Vec<_>>());
        assert!(!q.analysis[0].content.contains("根据向量垂直"), "思路摘要不应留在解析: {}", q.analysis[0].content);
        assert!(q.analysis[0].content.contains("因为"));
        assert!(!q.analysis[0].title.contains("解法二"), "合并后不应还叫解法二: {}", q.analysis[0].title);
    }

    #[test]
    fn recover_merges_even_if_llm_already_split() {
        let md = "\
7. 画出图象求交点个数\n\
【答案】C\n\
【解析】\n\
【分析】画出两函数在 $[0,2\\pi]$ 上的图象，根据图象即可求解。\n\
【详解】因为函数 $y=\\sin x$ 的最小正周期为 $2\\pi$，故选：C。\n";
        let mut q = parse_q(json!({
            "question_type": "choice",
            "stem": "7. 画出图象求交点个数",
            "analysis": [
                {"title": "解法一", "content": "画出两函数在 $[0,2\\pi]$ 上的图象，根据图象即可求解。"},
                {"title": "解法二", "content": "因为函数 $y=\\sin x$ 的最小正周期为 $2\\pi$，故选：C。"}
            ],
            "parts": []
        }));
        recover_question_sections(&mut q, md);
        assert_eq!(q.analysis.len(), 1, "{:?}", q.analysis.iter().map(|a| &a.title).collect::<Vec<_>>());
        assert!(!q.analysis[0].content.contains("【分析】"), "{}", q.analysis[0].content);
        assert!(!q.analysis[0].content.contains("画出两函数"), "【分析】摘要不应留在解析: {}", q.analysis[0].content);
        assert!(q.analysis[0].content.contains("故选"));
    }

    #[test]
    fn finalize_merges_analysis_opener_without_keqiu() {
        let mut q = parse_q(json!({
            "question_type": "fill",
            "stem": "离心率",
            "analysis": [
                {"title": "解法一", "content": "由题意画出双曲线大致图象，结合第一定义求 $a,b,c$."},
                {"title": "解法二", "content": "由题可知 $A,B,F$ 三点横坐标相等，设 $A$ 在第一象限，得 $e=\\dfrac{3}{2}$。"}
            ],
            "parts": []
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(q.analysis.len(), 1, "{:?}", q.analysis);
        assert!(q.analysis[0].content.contains("由题意画出"));
        assert!(q.analysis[0].content.contains("横坐标相等"));
    }

    #[test]
    fn does_not_merge_two_real_numbered_methods() {
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": "16. 椭圆",
            "analysis": [],
            "parts": [{
                "label": "(2)",
                "analyses": [
                    {"title": "法一", "content": "平移直线"},
                    {"title": "法二", "content": "点差法。设斜率 $k$ 后联立韦达。"}
                ]
            }]
        }));
        finalize_parsed_question(&mut q);
        assert_eq!(q.parts[0].analyses.len(), 2, "法一/法二不得合并");
    }

    #[test]
    fn split_keeps_conic_geometry_in_stem() {
        let md = "16. 本题考查解析几何中的椭圆。\n（1）求 $e$\n";
        let (body, tail) = split_body_and_tail(md);
        assert!(tail.is_empty(), "不得把「解析几何」当成解析栏");
        assert!(body.contains("解析几何"));
    }

    #[test]
    fn split_cuts_at_analysis_heading() {
        let md = lecture_md();
        let (body, tail) = split_body_and_tail(&md);
        assert!(body.contains("已知椭圆"));
        assert!(!body.contains("【解析】"));
        assert!(tail.contains("法六"));
        assert!(tail.contains("【小问2详解】"));
        let input = stage2_llm_input(&md);
        assert!(input.contains("（2）若过"));
        assert!(!input.contains("法六"));
    }

    #[test]
    fn recover_peels_dump_and_restores_six_methods() {
        let md = lecture_md();
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": md,
            "question_no": "16",
            "analysis": [{"title": "解法一", "content": ""}],
            "parts": []
        }));
        recover_question_sections(&mut q, &md);
        assert!(
            !q.stem.contains("【解析】") && !q.stem.contains("法六"),
            "题干应去掉解析: {}",
            q.stem
        );
        assert!(q.stem.contains("已知椭圆"));
        let p2 = q
            .parts
            .iter()
            .find(|p| label_number(&p.label) == Some(2))
            .expect("小问2");
        assert_eq!(p2.analyses.len(), 6, "{:?}", p2.analyses.iter().map(|a| &a.title).collect::<Vec<_>>());
        assert!(p2.analyses[5].content.contains("水平宽"));
        let p1 = q
            .parts
            .iter()
            .find(|p| label_number(&p.label) == Some(1))
            .expect("小问1");
        assert_eq!(p1.analyses.len(), 2);
    }

    #[test]
    fn recover_handles_spaced_subquestion_heading() {
        let md = "\
16. 椭圆\n\
（1）求 e\n\
【小问 1 详解】\n\
法一：计算 a、b\n\
【小问 2 详解】\n\
法一：平移\n\
法二：点差\n\
法三：韦达\n\
法四：参数\n\
法五：铅垂\n\
法六：水平宽\n";
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": md,
            "analysis": []
        }));
        recover_question_sections(&mut q, md);
        assert!(!q.stem.contains("详解"));
        let p2 = q
            .parts
            .iter()
            .find(|p| label_number(&p.label) == Some(2))
            .expect("小问2");
        assert_eq!(p2.analyses.len(), 6);
    }

    #[test]
    fn recover_prefers_ocr_methods_over_truncated_llm() {
        let md = lecture_md();
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": "16. 已知椭圆",
            "question_no": "16",
            "parts": [{
                "id": "a",
                "label": "(1)",
                "stem": "求离心率",
                "analyses": []
            }, {
                "id": "b",
                "label": "(2)",
                "stem": "求 l 的方程",
                "analyses": [
                    {"title": "法二", "content": "点差法残片"},
                    {"title": "法三", "content": "韦达残片"},
                    {"title": "法四", "content": "参数残片"}
                ]
            }]
        }));
        recover_question_sections(&mut q, &md);
        assert_eq!(q.stem, "16. 已知椭圆");
        assert_eq!(q.parts[1].analyses.len(), 6);
        assert!(q.parts[1].analyses.iter().any(|a| a.content.contains("水平宽")));
        assert_eq!(q.parts[0].analyses.len(), 2);
    }

    #[test]
    fn recover_fills_answers_from_answer_block() {
        let md = "\
16. 椭圆题\n\
（1）求 e\n\
【答案】\n\
（1）$e=\\dfrac{\\sqrt{3}}{2}$\n\
（2）$y=x+1$\n\
【解析】\n\
法一：计算离心率\n";
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": "16. 椭圆题",
            "parts": [
                {"label": "(1)", "stem": "求 e", "answer": "", "analyses": []},
                {"label": "(2)", "stem": "求 l", "answer": "", "analyses": []}
            ]
        }));
        recover_question_sections(&mut q, md);
        assert!(q.parts[0].answer.as_deref().unwrap_or("").contains("sqrt"));
        assert!(q.parts[1].answer.as_deref().unwrap_or("").contains("y=x+1"));
        assert!(
            q.parts.iter().any(|p| p.analyses.iter().any(|a| a.content.contains("离心率"))),
            "解析应回填到叶子"
        );
    }

    #[test]
    fn split_methods_cuts_inline_fa_wu_after_comma() {
        let text = "\
法四：参数方程联立得交点。$d=3$，法五：当 $l$ 的斜率不存在时，$l:x=3$，$B\\left(3,-\\dfrac{3}{2}\\right)$。\n\
法六：水平宽乘铅垂高。";
        let methods = split_methods(text);
        let titles: Vec<_> = methods.iter().map(|m| m.title.as_str()).collect();
        assert_eq!(titles, vec!["法四", "法五", "法六"], "{titles:?}");
        assert!(methods[1].content.contains("斜率不存在"));
        assert!(!methods[0].content.contains("法五"));
    }

    #[test]
    fn resplit_pulls_fa_wu_out_of_fa_si_content() {
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": "16. 椭圆",
            "parts": [{
                "label": "(2)",
                "analyses": [
                    {"title": "法四", "content": "参数方程。$d=3$，法五：当 $l$ 的斜率不存在时，$l:x=3$"},
                    {"title": "法六", "content": "水平宽乘铅垂高"}
                ]
            }]
        }));
        resplit_nested_methods(&mut q);
        let titles: Vec<_> = q.parts[0].analyses.iter().map(|a| a.title.as_str()).collect();
        assert_eq!(titles, vec!["法四", "法五", "法六"], "{titles:?}");
        assert!(q.parts[0].analyses[1].content.contains("斜率不存在"));
    }

    #[test]
    fn split_methods_does_not_cut_mention_without_colon() {
        let text = "法一：先用点差法，不同于法二的思路。\n法二：设斜率。";
        let methods = split_methods(text);
        assert_eq!(methods.len(), 2, "{:?}", methods.iter().map(|m| &m.title).collect::<Vec<_>>());
        assert!(methods[0].content.contains("不同于法二"));
    }

    #[test]
    fn peels_inline_answer_from_stem_and_strips_hash_score() {
        let md = "甲、乙按规则抽牌，求甲的胜率。【答案】$\\frac{1}{2}$ ##0.5\n【详解】\n将每局得分视为随机变量。";
        let mut q = parse_q(json!({
            "question_type": "solution",
            "stem": "甲、乙按规则抽牌，求甲的胜率。【答案】$\\frac{1}{2}$ ##0.5",
            "analysis": [{"title": "解法一", "content": "将每局得分视为随机变量。"}],
            "parts": []
        }));
        recover_question_sections(&mut q, md);
        assert!(
            !q.stem.contains("【答案】") && !q.stem.contains("frac"),
            "题干不应再含答案: {}",
            q.stem
        );
        assert!(q.stem.contains("求甲的胜率"));
        q.ensure_solution_parts();
        let ans = q.parts[0].answer.as_deref().unwrap_or("");
        assert!(ans.contains("frac"), "答案应写入叶子: {ans}");
        assert!(!ans.contains("##"), "应去掉 ##0.5: {ans}");
        assert!(!ans.contains("随机变量"), "详解不得写进答案栏: {ans}");
    }

    #[test]
    fn stage2_splits_short_answer_tail() {
        let md = "求甲的胜率。【答案】$\\frac{1}{2}$ ##0.5";
        let (body, tail) = split_body_and_tail(md);
        assert!(body.contains("求甲的胜率"));
        assert!(!body.contains("【答案】"));
        assert!(tail.contains("【答案】"));
        let input = stage2_llm_input(md);
        assert!(!input.contains("【答案】"), "短答案尾也不得整段送给 Stage2: {input}");
    }
}
