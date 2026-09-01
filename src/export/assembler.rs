//! 装配器（T1.4）— 实施计划 §5.1 / §十二（权限）
//!
//! `ExamRequest` → `ExamBundle`：批量取题（`WHERE id = ANY($1)` 一跳查库）→
//! 可见性过滤（与 `get_question` 同口径：`can_access_space` + Public 空间仅
//! Published + 管理员放行）→ 选项解析回退（移植 `Basket.vue` 的
//! `parseOptions` / `extractOptionsFromStem`）→ 填空挖空（B2）→ 问树展开 →
//! Callout 派生（教师/讲义模式）→ 连续题号重排。
//!
//! 不可见或不存在的题目跳过并记入卷级 `issues`（经 X-Export-Warnings 上报）。
//! 纯装配逻辑（`assemble_bundle`）与 DB 访问分离，便于单测。

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use regex::Regex;
use uuid::Uuid;

use crate::auth::middleware::AuthUser;
use crate::auth::permissions::{can_access_space, get_space, is_admin_user};
use crate::export::content::split_content;
use crate::export::model::{
    Callout, CalloutKind, CalloutOptions, ExamBundle, ExamOption, ExamQuestion, ExamRequest,
    ExamSection, ExportMode, InlineNode, Issue, IssueField, IssueSeverity, QuestionKind,
};
use crate::models::question::{Question, QuestionStatus, QuestionType};
use crate::models::question_structure::{
    parse_structure, walk_leaves, AnalysisBlock, QuestionPart,
};
use crate::models::space::{Space, SpaceKind};

/// 装配结果：ExamBundle + 卷级问题（题级问题在各 `ExamQuestion.issues`）
#[derive(Debug, Clone)]
pub struct AssembledExam {
    pub bundle: ExamBundle,
    pub issues: Vec<Issue>,
}

// ═══════════════════════════ 入口（含 DB 访问） ═══════════════════════════

/// 装配试卷：批量查库 + 可见性过滤 + 纯装配
pub async fn assemble_exam(
    pool: &sqlx::PgPool,
    auth: &AuthUser,
    req: &ExamRequest,
) -> Result<AssembledExam, sqlx::Error> {
    let ids: Vec<Uuid> = req
        .sections
        .iter()
        .flat_map(|s| s.questions.iter().map(|q| q.id))
        .collect();

    // 1. 批量取题
    let qmap: HashMap<Uuid, Question> = if ids.is_empty() {
        HashMap::new()
    } else {
        sqlx::query_as::<_, Question>("SELECT * FROM questions WHERE id = ANY($1)")
            .bind(&ids)
            .fetch_all(pool)
            .await?
            .into_iter()
            .map(|q| (q.id, q))
            .collect()
    };

    // 2. 空间缓存（去重加载，可见性判定需要）
    let mut spaces: HashMap<Uuid, Option<Space>> = HashMap::new();
    for q in qmap.values() {
        if !spaces.contains_key(&q.space_id) {
            spaces.insert(q.space_id, get_space(pool, q.space_id).await?);
        }
    }

    // 3. 可见性：与 get_question 完全同口径
    let mut visible: HashSet<Uuid> = HashSet::new();
    for q in qmap.values() {
        let can = match spaces.get(&q.space_id) {
            Some(Some(space)) => {
                can_access_space(pool, auth, space).await?
                    && (space.kind != SpaceKind::Public
                        || q.status == QuestionStatus::Published
                        || is_admin_user(auth))
            }
            // 空间不存在（已删除）→ 不可见
            _ => false,
        };
        if can {
            visible.insert(q.id);
        }
    }

    // 4. 教师模式预取 Callout 数据（知识点 / 易错标签，批量两跳）
    let want_callouts = req.mode == ExportMode::Teacher && {
        let c = &req.options.callouts;
        c.knowledge || c.error_prone || c.analysis
    };
    let (kn_map, ep_map) = if want_callouts {
        (
            fetch_knowledge_names(pool, &ids).await?,
            fetch_error_prone_tags(pool, &ids).await?,
        )
    } else {
        (HashMap::new(), HashMap::new())
    };

    Ok(assemble_bundle(req, &qmap, &visible, &kn_map, &ep_map))
}

/// 题目 → 知识点名称（is_primary 置顶，排序与 build_detail 一致）
async fn fetch_knowledge_names(
    pool: &sqlx::PgPool,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<String>>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT qkn.question_id, kn.name
        FROM question_knowledge_nodes qkn
        JOIN knowledge_nodes kn ON kn.id = qkn.node_id
        WHERE qkn.question_id = ANY($1)
        ORDER BY qkn.question_id, qkn.is_primary DESC, kn.sort_order, kn.name
        "#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;
    Ok(group_rows(rows))
}

/// 题目 → 易错点标签名（error_prone 类别）
async fn fetch_error_prone_tags(
    pool: &sqlx::PgPool,
    ids: &[Uuid],
) -> Result<HashMap<Uuid, Vec<String>>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        r#"
        SELECT qtr.question_id, t.name
        FROM question_tags_relation qtr
        JOIN tags t ON t.id = qtr.tag_id
        WHERE qtr.question_id = ANY($1) AND t.category = 'error_prone'
        ORDER BY qtr.question_id, t.name
        "#,
    )
    .bind(ids)
    .fetch_all(pool)
    .await?;
    Ok(group_rows(rows))
}

fn group_rows(rows: Vec<(Uuid, String)>) -> HashMap<Uuid, Vec<String>> {
    let mut map: HashMap<Uuid, Vec<String>> = HashMap::new();
    for (qid, name) in rows {
        map.entry(qid).or_default().push(name);
    }
    map
}

// ═══════════════════════════ 纯装配（单测覆盖） ═══════════════════════════

/// 按请求顺序装配：跳过不可见/不存在的题并记警告，连续题号跨大题重排。
/// `added` 排序模式（前端单分组形态）天然兼容——sections 是什么就装配什么。
fn assemble_bundle(
    req: &ExamRequest,
    qmap: &HashMap<Uuid, Question>,
    visible: &HashSet<Uuid>,
    kn_map: &HashMap<Uuid, Vec<String>>,
    ep_map: &HashMap<Uuid, Vec<String>>,
) -> AssembledExam {
    let mut issues: Vec<Issue> = Vec::new();
    let mut number: u32 = 0;
    let mut sections: Vec<ExamSection> = Vec::with_capacity(req.sections.len());

    for sec_req in &req.sections {
        let mut questions: Vec<ExamQuestion> = Vec::with_capacity(sec_req.questions.len());
        for qr in &sec_req.questions {
            let Some(question) = qmap.get(&qr.id) else {
                issues.push(skip_issue(qr.id, "题目不存在或已被删除"));
                continue;
            };
            if !visible.contains(&qr.id) {
                issues.push(skip_issue(qr.id, "无权查看该题目"));
                continue;
            }
            number += 1;
            questions.push(assemble_question(
                number,
                qr.default_score,
                question,
                req.mode,
                &req.options.callouts,
                kn_map.get(&question.id).map(Vec::as_slice).unwrap_or(&[]),
                ep_map.get(&question.id).map(Vec::as_slice).unwrap_or(&[]),
            ));
        }
        sections.push(ExamSection {
            title: sec_req.title.clone(),
            instruction: sec_req.instruction.clone(),
            questions,
        });
    }

    AssembledExam {
        bundle: ExamBundle {
            title: req.title.clone(),
            subtitle: req.subtitle.clone(),
            exam_meta: req.exam_meta.clone(),
            mode: req.mode,
            sections,
        },
        issues,
    }
}

fn skip_issue(id: Uuid, reason: &str) -> Issue {
    Issue {
        question_no: None,
        field: IssueField::Other,
        severity: IssueSeverity::Warning,
        latex: None,
        reason: format!("{}（id: {}），已跳过", reason, id),
    }
}

/// 单题装配：选项解析回退 → 填空挖空 → 切分 → Callout 派生
#[allow(clippy::too_many_arguments)]
fn assemble_question(
    number: u32,
    default_score: Option<f64>,
    q: &Question,
    mode: ExportMode,
    callout_opts: &CalloutOptions,
    knowledge_names: &[String],
    error_prone_tags: &[String],
) -> ExamQuestion {
    let letters = extract_choice_letters(q.correct_answer.as_ref());
    let kind = question_kind(&q.question_type, &letters);

    // 选项解析：options JSONB 优先；空时回退题干内嵌 A-D（仅选择题，≥2 项才生效）
    let mut stem = q.stem.clone();
    let mut raw_options = parse_options(q.options.as_ref());
    if raw_options.is_empty()
        && matches!(kind, QuestionKind::SingleChoice | QuestionKind::MultiChoice)
    {
        let (clean, extracted) = extract_options_from_stem(&stem);
        if extracted.len() >= 2 {
            stem = clean;
            raw_options = extracted;
        }
    }

    // 填空挖空（B2）：学生卷 stem 无 ______ 时，按 blanks 把内嵌答案挖为等长下划线
    let blanks = extract_fill_blanks(q.correct_answer.as_ref());
    if kind == QuestionKind::Fill && mode != ExportMode::Teacher && !stem.contains("____") {
        stem = hollow_fill_stem(&stem, &blanks);
    }

    let analyses = q
        .analysis
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(|a| {
            vec![AnalysisBlock {
                id: "analysis".to_string(),
                title: String::new(),
                content: a.to_string(),
            }]
        })
        .unwrap_or_default();

    // 问树展开：解答题 structure → parts（原始文本，生成期切分）
    let structure_parts: Vec<QuestionPart> = if q.question_type == QuestionType::Solution {
        parse_structure(q.structure.as_ref())
            .map(|s| s.parts)
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let callouts =
        derive_callouts(mode, callout_opts, knowledge_names, error_prone_tags, q, &structure_parts);

    ExamQuestion {
        number,
        score: resolve_score(default_score, q),
        kind,
        stem: split_content(&stem),
        options: raw_options
            .into_iter()
            .map(|o| ExamOption {
                label: o.label,
                content: split_content(&o.content),
            })
            .collect(),
        answers: assemble_answers(kind, &letters, &blanks, q),
        analyses,
        structure_parts,
        callouts,
        answer_space: None,
        issues: Vec::new(),
    }
}

/// 题型映射：与前端 `Basket.vue bucketType()` 对齐
/// （choice 按答案字母数升级为多选；DB 枚举无 composite，IR 保留完整性）
fn question_kind(t: &QuestionType, letters: &[String]) -> QuestionKind {
    match t {
        QuestionType::Choice => {
            if letters.len() > 1 {
                QuestionKind::MultiChoice
            } else {
                QuestionKind::SingleChoice
            }
        }
        QuestionType::Multiple => QuestionKind::MultiChoice,
        QuestionType::Fill => QuestionKind::Fill,
        QuestionType::Solution => QuestionKind::Solution,
    }
}

/// 分值：请求 default_score → metadata.default_score → 兜底 5 分
fn resolve_score(default_score: Option<f64>, q: &Question) -> f64 {
    default_score
        .or_else(|| q.metadata.get("default_score").and_then(|v| v.as_f64()))
        .unwrap_or(5.0)
}

/// 答案：选择题字母 / 填空逐空 / 解答题问树叶子答案
fn assemble_answers(
    kind: QuestionKind,
    letters: &[String],
    blanks: &[FillBlank],
    q: &Question,
) -> Vec<String> {
    match kind {
        QuestionKind::SingleChoice | QuestionKind::MultiChoice => letters.to_vec(),
        QuestionKind::Fill => blanks.iter().map(|b| b.answer.clone()).collect(),
        _ => parse_structure(q.structure.as_ref())
            .map(|s| {
                walk_leaves(&s.parts)
                    .iter()
                    .filter_map(|p| p.answer.as_deref())
                    .map(str::trim)
                    .filter(|a| !a.is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
    }
}

/// Callout 派生（仅教师/讲义模式；§5.1 四类）
fn derive_callouts(
    mode: ExportMode,
    opts: &CalloutOptions,
    knowledge_names: &[String],
    error_prone_tags: &[String],
    q: &Question,
    structure_parts: &[QuestionPart],
) -> Vec<Callout> {
    if mode != ExportMode::Teacher {
        return Vec::new();
    }
    let mut out = Vec::new();
    if opts.knowledge && !knowledge_names.is_empty() {
        out.push(Callout {
            kind: CalloutKind::Knowledge,
            title: "考点清单".to_string(),
            nodes: vec![InlineNode::Text {
                text: knowledge_names.join("、"),
            }],
        });
    }
    if opts.error_prone && !error_prone_tags.is_empty() {
        out.push(Callout {
            kind: CalloutKind::ErrorProne,
            title: "易错警示".to_string(),
            nodes: vec![InlineNode::Text {
                text: error_prone_tags.join("、"),
            }],
        });
    }
    if opts.analysis {
        if let Some(a) = q.analysis.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
            out.push(Callout {
                kind: CalloutKind::Tip,
                title: "名师点拨".to_string(),
                nodes: split_content(a),
            });
        }
        // 思路拆解：问树各解法块
        for part in walk_leaves(structure_parts) {
            for block in &part.analyses {
                let content = block.content.trim();
                if content.is_empty() {
                    continue;
                }
                out.push(Callout {
                    kind: CalloutKind::Approach,
                    title: if block.title.trim().is_empty() {
                        "思路拆解".to_string()
                    } else {
                        block.title.clone()
                    },
                    nodes: split_content(content),
                });
            }
        }
    }
    out
}

// ═══════════════════════════ 选项解析（移植 Basket.vue） ═══════════════════════════

/// 裸选项（label 可能未解析）
#[derive(Debug, Clone, PartialEq)]
struct RawOption {
    label: String,
    content: String,
}

/// `parseOptions`：兼容 JSON 字符串、`{label,content}` 对象数组、裸字符串数组
fn parse_options(raw: Option<&serde_json::Value>) -> Vec<RawOption> {
    let Some(v) = raw else {
        return Vec::new();
    };
    let parsed: serde_json::Value = match v {
        serde_json::Value::String(s) => match serde_json::from_str(s) {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        },
        other => other.clone(),
    };
    let serde_json::Value::Array(arr) = parsed else {
        return Vec::new();
    };
    arr.iter()
        .map(|opt| match opt {
            serde_json::Value::String(s) => {
                let label_re = option_label_re();
                if let Some(c) = label_re.captures(s) {
                    RawOption {
                        label: c[1].to_string(),
                        content: c[2].to_string(),
                    }
                } else {
                    RawOption {
                        label: String::new(),
                        content: s.clone(),
                    }
                }
            }
            serde_json::Value::Object(obj) => {
                if let Some(label) = obj.get("label").and_then(|l| l.as_str()) {
                    RawOption {
                        label: label.to_string(),
                        content: obj
                            .get("content")
                            .and_then(|c| c.as_str())
                            .unwrap_or("")
                            .to_string(),
                    }
                } else {
                    RawOption {
                        label: String::new(),
                        content: value_to_string(opt),
                    }
                }
            }
            other => RawOption {
                label: String::new(),
                content: value_to_string(other),
            },
        })
        .collect()
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// `extractOptionsFromStem`：题干内嵌 "A. … B. …" 回退解析。
/// 返回（清洗后的题干, 选项）；≥2 项才视为有效（由调用方判定）。
fn extract_options_from_stem(stem: &str) -> (String, Vec<RawOption>) {
    if stem.is_empty() {
        return (String::new(), Vec::new());
    }
    let Some(m) = option_start_re().find(stem) else {
        return (stem.to_string(), Vec::new());
    };
    let clean_stem = stem[..m.start()].trim().to_string();
    let section = stem[m.start()..].trim();

    // 逐个定位选项起点；内容 = 当前匹配结束 → 下一匹配开始（含其前缀）
    let re = option_item_re();
    let mut matches: Vec<regex::Match> = re.find_iter(&section).collect();
    if matches.is_empty() {
        return (stem.to_string(), Vec::new());
    }
    let mut options: Vec<RawOption> = Vec::new();
    while let Some(m0) = matches.first().cloned() {
        // 找出与 m0 同一起点的最长匹配（\s* 贪婪已由正则保证，此处直接取文本）
        let label = m0
            .as_str()
            .chars()
            .find(|c| c.is_ascii_alphabetic())
            .map(|c| c.to_ascii_uppercase())
            .unwrap_or_default();
        let content_end = matches.get(1).map(|next| next.start()).unwrap_or(section.len());
        let content = section[m0.end()..content_end].trim().to_string();
        if !content.is_empty() {
            options.push(RawOption {
                label: label.to_string(),
                content,
            });
        }
        matches.remove(0);
    }
    (clean_stem, options)
}

// ── 正则（OnceLock 单例；与 Basket.vue 的 JS 正则逐字对齐） ──

/// 选项区起点：`(?:^|\n|\s+)(?:[A-D][.、\s:：]|\([A-D]\))`（/i）
fn option_start_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:^|\n|\s+)(?:[A-Da-d][.、\s:：]|\([A-Da-d]\))").expect("valid regex")
    })
}

/// 选项项：`(?:^|\n|\s+|\b)([A-D])[.、\s:：)]\s*`（/i）
fn option_item_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:^|\n|\s+|\b)[A-Da-d][.、\s:：)]\s*").expect("valid regex")
    })
}

/// 选项字母前缀：`^([A-Z])[.、．\s]\s*(.*)$`
fn option_label_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^([A-Z])[.、．\s]\s*(.*)$").expect("valid regex"))
}

// ═══════════════════════════ 答案解析（移植 choiceAnswer.ts） ═══════════════════════════

/// 从 B / ['B'] / $\mathrm{B}$ / {options:['B']} 等形态抽出 A–D 字母（去重保序）
fn extract_choice_letters(raw: Option<&serde_json::Value>) -> Vec<String> {
    let mut out: Vec<char> = Vec::new();
    extract_letters_into(raw, &mut out);
    out.into_iter().map(String::from).collect()
}

fn extract_letters_into(raw: Option<&serde_json::Value>, out: &mut Vec<char>) {
    let Some(v) = raw else { return };
    match v {
        serde_json::Value::Null => {}
        serde_json::Value::Array(arr) => {
            for item in arr {
                extract_letters_into(Some(item), out);
            }
        }
        serde_json::Value::Object(obj) => {
            if let Some(opts) = obj.get("options") {
                if opts.is_array() {
                    extract_letters_into(Some(opts), out);
                    return;
                }
            }
            if let Some(val) = obj.get("value") {
                if val.is_object() {
                    extract_letters_into(Some(val), out);
                }
            }
        }
        serde_json::Value::String(s) => letters_from_string(s, out),
        _ => {}
    }
}

fn letters_from_string(s: &str, out: &mut Vec<char>) {
    let mathrm_re = mathrm_re();
    let (source, is_mathrm) = match mathrm_re.captures(s) {
        Some(c) => (c[1].to_string(), true),
        None => (s.to_string(), false),
    };
    let mut letters: Vec<char> = Vec::new();
    for ch in source.to_uppercase().chars() {
        if ('A'..='D').contains(&ch) && !letters.contains(&ch) {
            letters.push(ch);
        }
    }
    if is_mathrm {
        append_letters(&letters, out);
        return;
    }
    let trimmed = source.trim();
    // 单字母形态
    if trimmed.chars().count() == 1 {
        let c = trimmed.chars().next().unwrap().to_ascii_uppercase();
        if ('A'..='D').contains(&c) {
            append_letters(&[c], out);
            return;
        }
    }
    // 去空白与分隔符后为纯 A-D 串
    let compact: String = trimmed
        .chars()
        .filter(|c| !matches!(c, ' ' | '\t' | '\n' | '\r' | ',' | '，' | '、' | '$' | '\\' | '{' | '}'))
        .collect();
    if !compact.is_empty() && compact.chars().all(|c| ('A'..='D').contains(&c)) {
        append_letters(&letters, out);
        return;
    }
    // 短文本：保留扫描到的字母；长文本视为普通文字
    if trimmed.chars().count() <= 12 {
        append_letters(&letters, out);
    }
}

fn append_letters(letters: &[char], out: &mut Vec<char>) {
    for c in letters {
        if !out.contains(c) {
            out.push(*c);
        }
    }
}

/// `\\mathrm{...}` 提取
fn mathrm_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\\mathrm\s*\{([A-Za-z]+)\}").expect("valid regex"))
}

/// 填空答案：{position, answer}
#[derive(Debug, Clone, PartialEq)]
struct FillBlank {
    position: usize,
    answer: String,
}

/// `extractFillBlanks`：['a'] / [{position,answer}] / {blanks:[…]} / {value:{…}}
fn extract_fill_blanks(raw: Option<&serde_json::Value>) -> Vec<FillBlank> {
    let Some(v) = raw else {
        return Vec::new();
    };
    match v {
        serde_json::Value::Array(arr) => {
            let mut out = Vec::new();
            for (i, item) in arr.iter().enumerate() {
                if let serde_json::Value::Object(obj) = item {
                    let answer = obj
                        .get("answer")
                        .map(value_to_string)
                        .unwrap_or_default()
                        .trim()
                        .to_string();
                    if answer.is_empty() {
                        continue;
                    }
                    let position = obj
                        .get("position")
                        .and_then(|p| p.as_u64())
                        .map(|p| p as usize)
                        .unwrap_or(i + 1);
                    out.push(FillBlank { position, answer });
                } else {
                    let s = value_to_string(item).trim().to_string();
                    if !s.is_empty() {
                        out.push(FillBlank {
                            position: i + 1,
                            answer: s,
                        });
                    }
                }
            }
            out
        }
        serde_json::Value::Object(obj) => {
            if let Some(b) = obj.get("blanks") {
                if b.is_array() {
                    return extract_fill_blanks(Some(b));
                }
            }
            if let Some(val) = obj.get("value") {
                return extract_fill_blanks(Some(val));
            }
            Vec::new()
        }
        _ => Vec::new(),
    }
}

// ═══════════════════════════ 填空挖空（B2） ═══════════════════════════

/// stem 已含 ______（≥4 连续下划线）直接沿用；
/// 否则把内嵌在 stem 中的答案文本替换为等长下划线（最短 4，保证空位可见）。
fn hollow_fill_stem(stem: &str, blanks: &[FillBlank]) -> String {
    let mut out = stem.to_string();
    for b in blanks {
        if b.answer.is_empty() || !out.contains(&b.answer) {
            continue;
        }
        let width = b.answer.chars().count().max(4);
        out = out.replacen(&b.answer, &"_".repeat(width), 1);
    }
    out
}

// ═══════════════════════════ 单元测试 ═══════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::json;
    use uuid::Uuid;

    /// 构造可自由覆盖少量字段的 Question（其余默认）
    fn mq(id: Uuid, question_type: QuestionType, stem: &str) -> Question {
        Question {
            id,
            stem: stem.to_string(),
            stem_text: None,
            images: None,
            question_type,
            options: None,
            correct_answer: None,
            analysis: None,
            structure: None,
            difficulty: crate::models::question::Difficulty(3),
            metadata: json!({}),
            content_hash: None,
            normalized_content_hash: None,
            parent_id: None,
            sub_order: None,
            paper_count: 0,
            attempt_count: 0,
            accuracy_rate: None,
            favorite_count: 0,
            status: QuestionStatus::Published,
            space_id: Uuid::new_v4(),
            origin_question_id: None,
            creator_id: Uuid::new_v4(),
            created_at: Utc::now(),
            updated_by: None,
            updated_at: Utc::now(),
            version: 1,
        }
    }

    fn req(mode: ExportMode, sections: Vec<ExamSectionRequest>) -> ExamRequest {
        ExamRequest {
            title: "测试卷".to_string(),
            subtitle: None,
            exam_meta: Default::default(),
            mode,
            sections,
            options: Default::default(),
            spec: None,
        }
    }

    use crate::export::model::ExamSectionRequest;
    use crate::export::model::ExamQuestionRequest;

    // ── parse_options ──

    #[test]
    fn parse_options_object_array() {
        let raw = json!([
            {"label": "A", "content": "2 个"},
            {"label": "B", "content": "3 个"},
        ]);
        let opts = parse_options(Some(&raw));
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].label, "A");
        assert_eq!(opts[1].content, "3 个");
    }

    #[test]
    fn parse_options_string_array_with_labels() {
        let raw = json!(["A. 2 个", "B、3 个", "C 4 个"]);
        let opts = parse_options(Some(&raw));
        assert_eq!(opts.len(), 3);
        assert_eq!(opts[0].label, "A");
        assert_eq!(opts[0].content, "2 个");
        assert_eq!(opts[1].label, "B");
        assert_eq!(opts[2].label, "C");
    }

    #[test]
    fn parse_options_bare_string_keeps_content() {
        let raw = json!(["2 个", "3 个"]);
        let opts = parse_options(Some(&raw));
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].label, "");
        assert_eq!(opts[0].content, "2 个");
    }

    #[test]
    fn parse_options_json_string_and_invalid() {
        // options 列有时以 JSON 字符串形态存储
        let raw = json!(r#"[{"label":"A","content":"甲"}]"#);
        let opts = parse_options(Some(&raw));
        assert_eq!(opts.len(), 1);
        assert_eq!(opts[0].label, "A");
        // 非 JSON 字符串 / 非数组 → 空
        assert!(parse_options(Some(&json!("不是 JSON"))).is_empty());
        assert!(parse_options(Some(&json!({"a":1}))).is_empty());
        assert!(parse_options(None).is_empty());
    }

    // ── extract_options_from_stem ──

    #[test]
    fn extract_options_inline_spaces() {
        let stem = "下列说法正确的是 A. 甲 B. 乙 C. 丙 D. 丁";
        let (clean, opts) = extract_options_from_stem(stem);
        assert_eq!(opts.len(), 4);
        assert_eq!(opts[0].label, "A");
        assert_eq!(opts[2].content, "丙");
        assert_eq!(clean, "下列说法正确的是");
    }

    #[test]
    fn extract_options_newline_and_paren_form() {
        let stem = "求值：\n(A) 1\n(B) 2";
        let (clean, opts) = extract_options_from_stem(stem);
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0].label, "A");
        assert_eq!(opts[1].label, "B");
        assert_eq!(opts[1].content, "2");
        // 与 Basket.vue JS 版逐字一致：内容正则的下一项前缀只认 [A-D] 起头，
        // "(A)" 形态的尾部 "\n(" 会残留在上一项 content 中（回退解析本为尽力而为）
        assert_eq!(opts[0].content, "1\n(");
        assert_eq!(clean, "求值：");
    }

    #[test]
    fn extract_options_fewer_than_two_returns_original() {
        let stem = "只有一项 A. 孤独的选项";
        let (clean, opts) = extract_options_from_stem(stem);
        // JS 版仅一项时返回原 stem（≥2 才生效由调用方判定，这里确认解析本身）
        assert_eq!(opts.len(), 1);
        assert_eq!(clean, "只有一项");
        // 无选项形态
        let (clean2, opts2) = extract_options_from_stem("普通题干没有选项");
        assert!(opts2.is_empty());
        assert_eq!(clean2, "普通题干没有选项");
    }

    // ── extract_choice_letters ──

    #[test]
    fn choice_letters_various_shapes() {
        assert_eq!(extract_choice_letters(Some(&json!("B"))), vec!["B"]);
        assert_eq!(
            extract_choice_letters(Some(&json!(["A", "C"]))),
            vec!["A", "C"]
        );
        assert_eq!(
            extract_choice_letters(Some(&json!({"options": ["B", "D"]}))),
            vec!["B", "D"]
        );
        assert_eq!(
            extract_choice_letters(Some(&json!({"kind": "choice", "value": {"options": ["AC"]}}))),
            vec!["A", "C"]
        );
        assert_eq!(
            extract_choice_letters(Some(&json!("$\\mathrm{BD}$"))),
            vec!["B", "D"]
        );
        assert_eq!(extract_choice_letters(Some(&json!("B, D"))), vec!["B", "D"]);
        // 长普通文本不含有效答案
        assert!(extract_choice_letters(Some(&json!("这是一段很长的中文描述文本内容超过十二个字符了"))).is_empty());
        assert!(extract_choice_letters(None).is_empty());
        assert!(extract_choice_letters(Some(&json!(""))).is_empty());
    }

    // ── extract_fill_blanks ──

    #[test]
    fn fill_blanks_various_shapes() {
        assert_eq!(
            extract_fill_blanks(Some(&json!({"blanks": [{"position": 1, "answer": "4"}]}))),
            vec![FillBlank { position: 1, answer: "4".into() }]
        );
        assert_eq!(
            extract_fill_blanks(Some(&json!(["甲", "乙"]))),
            vec![
                FillBlank { position: 1, answer: "甲".into() },
                FillBlank { position: 2, answer: "乙".into() }
            ]
        );
        // 对象缺 position → 序号兜底；空 answer 跳过
        assert_eq!(
            extract_fill_blanks(Some(&json!({"value": {"blanks": [{"answer": "x"}, {"answer": " "}]}}))),
            vec![FillBlank { position: 1, answer: "x".into() }]
        );
        assert!(extract_fill_blanks(None).is_empty());
    }

    // ── hollow_fill_stem ──

    #[test]
    fn hollow_keeps_existing_blanks() {
        let stem = "集合 A 的子集个数为______个";
        let out = hollow_fill_stem(stem, &[FillBlank { position: 1, answer: "4".into() }]);
        assert_eq!(out, stem);
    }

    #[test]
    fn hollow_replaces_embedded_answer() {
        let out = hollow_fill_stem(
            "函数 f(x) 的最小值是 4，最大值是 9",
            &[
                FillBlank { position: 1, answer: "4".into() },
                FillBlank { position: 2, answer: "9".into() },
            ],
        );
        // 答案长度 < 4 时按最短 4 个下划线挖空（保证空位可见）
        assert_eq!(out, "函数 f(x) 的最小值是 ____，最大值是 ____");
        assert!(!out.contains('4'));
    }

    #[test]
    fn hollow_answer_not_in_stem_noop() {
        let out = hollow_fill_stem("求 f(x) 的解析式", &[FillBlank { position: 1, answer: "y=x".into() }]);
        assert_eq!(out, "求 f(x) 的解析式");
    }

    // ── question_kind ──

    #[test]
    fn kind_mapping() {
        assert_eq!(
            question_kind(&QuestionType::Choice, &["B".into()]),
            QuestionKind::SingleChoice
        );
        // choice + 多字母 → 多选（与前端 bucketType 一致）
        assert_eq!(
            question_kind(&QuestionType::Choice, &["B".into(), "D".into()]),
            QuestionKind::MultiChoice
        );
        assert_eq!(
            question_kind(&QuestionType::Multiple, &[]),
            QuestionKind::MultiChoice
        );
        assert_eq!(question_kind(&QuestionType::Fill, &[]), QuestionKind::Fill);
        assert_eq!(
            question_kind(&QuestionType::Solution, &[]),
            QuestionKind::Solution
        );
    }

    // ── assemble_bundle ──

    #[test]
    fn assemble_renumbers_across_sections_and_skips_invisible() {
        let q1 = mq(Uuid::new_v4(), QuestionType::Fill, "第一题____");
        let q2 = mq(Uuid::new_v4(), QuestionType::Choice, "第二题");
        let q3 = mq(Uuid::new_v4(), QuestionType::Solution, "第三题");
        let missing = Uuid::new_v4();
        let (q1_id, q2_id, q3_id) = (q1.id, q2.id, q3.id);

        let mut qmap = HashMap::new();
        qmap.insert(q1.id, q1);
        qmap.insert(q2.id, q2);
        qmap.insert(q3.id, q3);
        let visible: HashSet<Uuid> = [q1_id, q3_id].into_iter().collect();

        let request = req(
            ExportMode::Student,
            vec![
                ExamSectionRequest {
                    title: "一、填空题".into(),
                    instruction: None,
                    questions: vec![
                        ExamQuestionRequest { id: q1_id, default_score: Some(3.0) },
                        ExamQuestionRequest { id: q2_id, default_score: None },
                    ],
                },
                ExamSectionRequest {
                    title: "二、解答题".into(),
                    instruction: Some("写出必要步骤".into()),
                    questions: vec![
                        ExamQuestionRequest { id: missing, default_score: None },
                        ExamQuestionRequest { id: q3_id, default_score: None },
                    ],
                },
            ],
        );

        let out = assemble_bundle(&request, &qmap, &visible, &HashMap::new(), &HashMap::new());
        // 不可见 q2 跳过 + missing 跳过 → 两条卷级警告
        assert_eq!(out.issues.len(), 2);
        assert!(out.issues.iter().all(|i| i.question_no.is_none()));
        assert!(out.issues[0].reason.contains("无权查看"));
        assert!(out.issues[1].reason.contains("不存在"));
        // 连续题号：1、2（跨大题，跳过的不占号）
        let nums: Vec<u32> = out
            .bundle
            .sections
            .iter()
            .flat_map(|s| s.questions.iter().map(|q| q.number))
            .collect();
        assert_eq!(nums, vec![1, 2]);
        // 分值：显式 > metadata > 兜底 5
        assert_eq!(out.bundle.sections[0].questions[0].score, 3.0);
        assert_eq!(out.bundle.sections[1].questions[0].score, 5.0);
        assert_eq!(out.bundle.sections[1].instruction.as_deref(), Some("写出必要步骤"));
    }

    #[test]
    fn assemble_added_single_section_shape() {
        // 前端「按加入顺序」是单分组形态：一个 section 装全部题
        let qs: Vec<Question> = (0..3)
            .map(|i| mq(Uuid::new_v4(), QuestionType::Choice, &format!("题干 {}", i)))
            .collect();
        let ids: Vec<Uuid> = qs.iter().map(|q| q.id).collect();
        let qmap: HashMap<Uuid, Question> = qs.into_iter().map(|q| (q.id, q)).collect();
        let visible: HashSet<Uuid> = ids.iter().copied().collect();

        let request = req(
            ExportMode::Student,
            vec![ExamSectionRequest {
                title: "按加入顺序".into(),
                instruction: None,
                questions: ids
                    .into_iter()
                    .map(|id| ExamQuestionRequest { id, default_score: None })
                    .collect(),
            }],
        );
        let out = assemble_bundle(&request, &qmap, &visible, &HashMap::new(), &HashMap::new());
        assert_eq!(out.bundle.sections.len(), 1);
        assert_eq!(out.bundle.sections[0].questions.len(), 3);
        assert!(out.issues.is_empty());
    }

    #[test]
    fn assemble_choice_with_stem_fallback_and_multi_letters() {
        let mut q = mq(Uuid::new_v4(), QuestionType::Choice, "正确的是 A. 甲 B. 乙 C. 丙 D. 丁");
        q.options = None;
        q.correct_answer = Some(json!({"kind": "choice", "value": {"options": ["B", "D"]}}));
        let visible: HashSet<Uuid> = [q.id].into_iter().collect();
        let qmap = HashMap::from([(q.id, q)]);

        let request = req(
            ExportMode::Student,
            vec![ExamSectionRequest {
                title: "一、选择题".into(),
                instruction: None,
                questions: vec![ExamQuestionRequest { id: *qmap.keys().next().unwrap(), default_score: None }],
            }],
        );
        let out = assemble_bundle(&request, &qmap, &visible, &HashMap::new(), &HashMap::new());
        let eq = &out.bundle.sections[0].questions[0];
        // 多字母 → 多选
        assert_eq!(eq.kind, QuestionKind::MultiChoice);
        // 选项从题干回退解析，题干清洗后不含选项
        assert_eq!(eq.options.len(), 4);
        assert_eq!(eq.options[1].label, "B");
        let stem_text = match &eq.stem[0] {
            InlineNode::Text { text } => text.clone(),
            other => panic!("unexpected stem head: {:?}", other),
        };
        assert_eq!(stem_text, "正确的是");
        assert_eq!(eq.answers, vec!["B", "D"]);
    }

    #[test]
    fn assemble_fill_hollow_only_for_student_paper() {
        let id = Uuid::new_v4();
        let mut q = mq(id, QuestionType::Fill, "f(x) 的最小值是 4");
        q.correct_answer = Some(json!({"kind": "fill", "value": {"blanks": [{"position": 1, "answer": "4"}]}}));
        let qmap = HashMap::from([(id, q)]);
        let visible: HashSet<Uuid> = [id].into_iter().collect();

        for (mode, expect_hollow) in [
            (ExportMode::Student, true),
            (ExportMode::Exam, true),
            (ExportMode::Teacher, false),
        ] {
            let request = req(
                mode,
                vec![ExamSectionRequest {
                    title: "一、填空题".into(),
                    instruction: None,
                    questions: vec![ExamQuestionRequest { id, default_score: None }],
                }],
            );
            let out = assemble_bundle(&request, &qmap, &visible, &HashMap::new(), &HashMap::new());
            let eq = &out.bundle.sections[0].questions[0];
            let hollowed = eq
                .stem
                .iter()
                .any(|n| matches!(n, InlineNode::Text { text } if text.contains("____")));
            assert_eq!(hollowed, expect_hollow, "mode {:?}", mode);
            // 挖空后答案仍在 answers 中
            assert_eq!(eq.answers, vec!["4"]);
        }
    }

    #[test]
    fn assemble_solution_tree_and_teacher_callouts() {
        let id = Uuid::new_v4();
        let mut q = mq(id, QuestionType::Solution, "已知函数 $f(x)=x^2$。");
        q.analysis = Some("先求导再讨论单调性。".to_string());
        q.structure = Some(json!({
            "version": 1,
            "parts": [{
                "id": "p1", "label": "(1)", "stem": "求单调区间",
                "children": [],
                "answer": "(-∞, 0) 递减",
                "analyses": [{"id": "a1", "title": "解法一：求导", "content": "f'(x)=2x"}],
                "no_analysis_needed": false, "label_dirty": false
            }]
        }));
        let qmap = HashMap::from([(id, q)]);
        let visible: HashSet<Uuid> = [id].into_iter().collect();
        let kn = HashMap::from([(id, vec!["导数的应用".to_string(), "二次函数".to_string()])]);
        let ep = HashMap::from([(id, vec!["忽略定义域".to_string()])]);

        let request = req(
            ExportMode::Teacher,
            vec![ExamSectionRequest {
                title: "一、解答题".into(),
                instruction: None,
                questions: vec![ExamQuestionRequest { id, default_score: None }],
            }],
        );
        let out = assemble_bundle(&request, &qmap, &visible, &kn, &ep);
        let eq = &out.bundle.sections[0].questions[0];

        assert_eq!(eq.kind, QuestionKind::Solution);
        assert_eq!(eq.structure_parts.len(), 1);
        assert_eq!(eq.answers, vec!["(-∞, 0) 递减"]);
        // 题级解析块来自 analysis 字段
        assert_eq!(eq.analyses.len(), 1);
        assert_eq!(eq.analyses[0].content, "先求导再讨论单调性。");

        // 四类 Callout 全部派生
        let kinds: Vec<CalloutKind> = eq.callouts.iter().map(|c| c.kind).collect();
        assert_eq!(
            kinds,
            vec![
                CalloutKind::Knowledge,
                CalloutKind::ErrorProne,
                CalloutKind::Tip,
                CalloutKind::Approach
            ]
        );
        let kn = eq.callouts[0].clone();
        assert_eq!(kn.title, "考点清单");
        assert!(matches!(kn.nodes[0], InlineNode::Text { ref text } if text == "导数的应用、二次函数"));
        let tip = eq.callouts[2].clone();
        assert_eq!(tip.title, "名师点拨");
        let approach = eq.callouts[3].clone();
        assert_eq!(approach.title, "解法一：求导");
    }

    #[test]
    fn teacher_callouts_respect_switches_and_student_mode_none() {
        let id = Uuid::new_v4();
        let mut q = mq(id, QuestionType::Choice, "题干");
        q.analysis = Some("解析".to_string());
        let visible: HashSet<Uuid> = [id].into_iter().collect();
        let kn = HashMap::from([(id, vec!["考点".to_string()])]);

        let mut request = req(
            ExportMode::Teacher,
            vec![ExamSectionRequest {
                title: "一".into(),
                instruction: None,
                questions: vec![ExamQuestionRequest { id, default_score: None }],
            }],
        );
        // 关掉 knowledge 与 analysis → 只剩无（本题无易错标签）
        request.options.callouts = CalloutOptions {
            knowledge: false,
            error_prone: true,
            analysis: false,
        };
        let qmap = HashMap::from([(id, q.clone())]);
        let out = assemble_bundle(&request, &qmap, &visible, &kn, &HashMap::new());
        assert!(out.bundle.sections[0].questions[0].callouts.is_empty());

        // 学生卷不派生 Callout
        let mut student_req = req(
            ExportMode::Student,
            vec![ExamSectionRequest {
                title: "一".into(),
                instruction: None,
                questions: vec![ExamQuestionRequest { id, default_score: None }],
            }],
        );
        student_req.options.callouts = CalloutOptions::default();
        let out = assemble_bundle(&student_req, &qmap, &visible, &kn, &HashMap::new());
        assert!(out.bundle.sections[0].questions[0].callouts.is_empty());
    }

    #[test]
    fn score_falls_back_to_metadata() {
        let id = Uuid::new_v4();
        let mut q = mq(id, QuestionType::Choice, "题干");
        q.metadata = json!({"default_score": 8});
        let visible: HashSet<Uuid> = [id].into_iter().collect();
        let request = req(
            ExportMode::Student,
            vec![ExamSectionRequest {
                title: "一".into(),
                instruction: None,
                questions: vec![ExamQuestionRequest { id, default_score: None }],
            }],
        );
        let out = assemble_bundle(
            &request,
            &HashMap::from([(id, q)]),
            &visible,
            &HashMap::new(),
            &HashMap::new(),
        );
        assert_eq!(out.bundle.sections[0].questions[0].score, 8.0);
    }
}
