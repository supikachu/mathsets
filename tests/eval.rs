// ============================================================
// AI 解析质量评测（Golden Dataset + F1）
// ------------------------------------------------------------
// 目标：评估 Stage 2 JSON 清洗 + 结构化解析管线（clean_and_parse）的健壮性。
//
// 评测范围（确定性、可离线复现）：
//   - clean_llm_json：代码块剥壳 / 尾逗号移除
//   - fix_invalid_escapes：LaTeX 非法转义修复（\sqrt \dfrac 等）
//   - extract_json_by_bracket_count：题干含 { } 时的花括号计数器
//   - ParsedQuestion 字段反序列化：题型 / 题干 / 选项 / 答案 / 知识点 / 配图
//
// 不在本次评测范围（需消耗 LLM 配额，非确定性）：
//   - Stage 1 OCR 引擎质量（Qwen-VL / Doc2X / MinerU）
//   - Stage 2 LLM 内容正确性（依赖模型能力）
//
// 运行：cargo test --test eval
// ============================================================

use mathset::ai::cleaner::clean_and_parse;
use mathset::ai::types::ParsedAnswer;
use mathset::ai::types::ParsedQuestion;
use serde::Deserialize;

// 将 Golden Dataset 编译期内联，避免运行时文件路径依赖
const GOLDEN_DATASET_JSON: &str = include_str!("eval/golden_dataset.json");

/// 整体通过阈值：字段准确率低于此值判定评测失败
const ACCURACY_THRESHOLD: f64 = 0.9;

// ---------------------------------------------------------------------------
// Golden Dataset 结构定义
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct GoldenCase {
    id: String,
    #[allow(dead_code)]
    description: String,
    raw_llm_output: String,
    expected: ExpectedFields,
}

#[derive(Debug, Deserialize)]
struct ExpectedFields {
    question_type: String,
    #[serde(default)]
    stem_contains: Option<String>,
    #[serde(default)]
    options_count: Option<usize>,
    #[serde(default)]
    answer_kind: Option<String>,
    #[serde(default)]
    answer_options: Option<Vec<String>>,
    #[serde(default)]
    blanks_count: Option<usize>,
    #[serde(default)]
    subs_count: Option<usize>,
    #[serde(default)]
    image_placeholders: Option<Vec<String>>,
    #[serde(default)]
    image_urls: Option<Vec<String>>,
    #[serde(default)]
    knowledge_points: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// 单条评测结果
// ---------------------------------------------------------------------------

struct CaseReport {
    id: String,
    parse_ok: bool,
    field_checks: Vec<(&'static str, bool)>,
    kp_precision: f64,
    kp_recall: f64,
    kp_f1: f64,
}

impl CaseReport {
    fn accuracy(&self) -> f64 {
        if self.field_checks.is_empty() {
            return 0.0;
        }
        let passed = self.field_checks.iter().filter(|(_, ok)| *ok).count();
        passed as f64 / self.field_checks.len() as f64
    }
}

// ---------------------------------------------------------------------------
// 比对辅助
// ---------------------------------------------------------------------------

fn knowledge_points_f1(
    predicted: &[String],
    expected: &[String],
) -> (f64, f64, f64) {
    // 精确匹配（大小写敏感）
    let pred_set: std::collections::HashSet<&String> = predicted.iter().collect();
    let exp_set: std::collections::HashSet<&String> = expected.iter().collect();

    let tp = pred_set.intersection(&exp_set).count();
    let precision = if pred_set.is_empty() {
        if exp_set.is_empty() { 1.0 } else { 0.0 }
    } else {
        tp as f64 / pred_set.len() as f64
    };
    let recall = if exp_set.is_empty() {
        1.0
    } else {
        tp as f64 / exp_set.len() as f64
    };
    let f1 = if precision + recall == 0.0 {
        0.0
    } else {
        2.0 * precision * recall / (precision + recall)
    };
    (precision, recall, f1)
}

fn check_answer_options(actual: &ParsedAnswer, expected: &[String]) -> bool {
    match actual {
        ParsedAnswer::Choice { options } => options == expected,
        _ => false,
    }
}

fn answer_kind_str(a: &ParsedAnswer) -> &'static str {
    match a {
        ParsedAnswer::Choice { .. } => "choice",
        ParsedAnswer::Fill { .. } => "fill",
        ParsedAnswer::Solution { .. } => "solution",
    }
}

// ---------------------------------------------------------------------------
// 主评测流程
// ---------------------------------------------------------------------------

fn run_eval() -> Vec<CaseReport> {
    let cases: Vec<GoldenCase> =
        serde_json::from_str(GOLDEN_DATASET_JSON).expect("golden_dataset.json 解析失败");

    let mut reports = Vec::with_capacity(cases.len());

    for case in cases {
        let mut report = CaseReport {
            id: case.id.clone(),
            parse_ok: false,
            field_checks: Vec::new(),
            kp_precision: 0.0,
            kp_recall: 0.0,
            kp_f1: 0.0,
        };

        // 1. 解析（核心被测管线：clean_and_parse）
        let parsed: ParsedQuestion = match clean_and_parse(&case.raw_llm_output) {
            Ok(p) => {
                report.parse_ok = true;
                p
            }
            Err(e) => {
                eprintln!("[{}] clean_and_parse 失败: {e}", case.id);
                report
                    .field_checks
                    .push(("parse_success", false));
                // 仍计入报告（accuracy = 0）
                let (p, r, f1) = knowledge_points_f1(&[], &[]);
                report.kp_precision = p;
                report.kp_recall = r;
                report.kp_f1 = f1;
                reports.push(report);
                continue;
            }
        };

        let exp = &case.expected;
        let mut checks = Vec::new();
        checks.push(("question_type", parsed.question_type == exp.question_type));

        if let Some(needle) = &exp.stem_contains {
            checks.push(("stem_contains", parsed.stem.contains(needle)));
        }
        if let Some(n) = &exp.options_count {
            let actual_n = parsed.options.as_ref().map(|o| o.len()).unwrap_or(0);
            checks.push(("options_count", actual_n == *n));
        }
        if let Some(kind) = &exp.answer_kind {
            let ans = parsed.correct_answer.as_ref().expect("golden dataset 有答案");
            checks.push(("answer_kind", answer_kind_str(ans) == kind));
        }
        if let Some(opts) = &exp.answer_options {
            let ans = parsed.correct_answer.as_ref().expect("golden dataset 有答案");
            checks.push(("answer_options", check_answer_options(ans, opts)));
        }
        if let Some(n) = &exp.blanks_count {
            let actual = match parsed.correct_answer.as_ref() {
                Some(ParsedAnswer::Fill { blanks }) => blanks.len(),
                _ => 0,
            };
            checks.push(("blanks_count", actual == *n));
        }
        if let Some(n) = &exp.subs_count {
            let actual = match parsed.correct_answer.as_ref() {
                Some(ParsedAnswer::Solution { subs }) => subs.len(),
                _ => 0,
            };
            checks.push(("subs_count", actual == *n));
        }
        if let Some(phs) = &exp.image_placeholders {
            checks.push(("image_placeholders", &parsed.image_placeholders == phs));
        }
        if let Some(urls) = &exp.image_urls {
            checks.push(("image_urls", &parsed.image_urls == urls));
        }

        // 知识点 F1（不计入 field_checks，单独统计）
        let expected_kp = exp.knowledge_points.clone().unwrap_or_default();
        let (p, r, f1) = knowledge_points_f1(&parsed.knowledge_points, &expected_kp);
        report.kp_precision = p;
        report.kp_recall = r;
        report.kp_f1 = f1;

        report.field_checks = checks;
        reports.push(report);
    }

    reports
}

// ---------------------------------------------------------------------------
// 评测测试入口
// ---------------------------------------------------------------------------

#[test]
fn eval_golden_dataset_overall_accuracy() {
    let reports = run_eval();

    let mut total_passed = 0usize;
    let mut total_run = 0usize;
    let mut all_parse_ok = true;

    println!("\n========== AI 解析质量评测报告 ==========");
    println!(
        "{:<32} {:<10} {:<10} {:<10} {:<10} {:<10}",
        "用例 ID", "解析", "字段准确率", "KP-P", "KP-R", "KP-F1"
    );
    println!("{}", "-".repeat(92));

    for r in &reports {
        let passed = r.field_checks.iter().filter(|(_, ok)| *ok).count();
        let run = r.field_checks.len();
        total_passed += passed;
        total_run += run;
        if !r.parse_ok {
            all_parse_ok = false;
        }
        println!(
            "{:<32} {:<10} {:<9.1}% {:<10.2} {:<10.2} {:<10.2}",
            r.id,
            if r.parse_ok { "✓" } else { "✗" },
            r.accuracy() * 100.0,
            r.kp_precision,
            r.kp_recall,
            r.kp_f1
        );
    }

    let overall_accuracy = if total_run == 0 {
        0.0
    } else {
        total_passed as f64 / total_run as f64
    };
    let overall_kp_f1 = {
        let mean: f64 = reports.iter().map(|r| r.kp_f1).sum::<f64>() / reports.len() as f64;
        mean
    };

    println!("{}", "-".repeat(92));
    println!(
        "整体字段准确率: {:.2}%  ({}/{})",
        overall_accuracy * 100.0,
        total_passed,
        total_run
    );
    println!("知识点 F1 均值: {:.3}", overall_kp_f1);
    println!("全部解析成功: {}", all_parse_ok);
    println!("=========================================\n");

    // 断言：字段准确率不低于阈值
    assert!(
        overall_accuracy >= ACCURACY_THRESHOLD,
        "字段准确率 {:.2}% 低于阈值 {:.0}%",
        overall_accuracy * 100.0,
        ACCURACY_THRESHOLD * 100.0
    );
    // 断言：所有用例必须解析成功（clean_and_parse 不允许失败）
    assert!(all_parse_ok, "存在解析失败的用例");
    // 断言：知识点 F1 均值不低于 0.85（允许部分模糊匹配不完美）
    assert!(
        overall_kp_f1 >= 0.85,
        "知识点 F1 均值 {overall_kp_f1:.3} 低于阈值 0.85"
    );
}

#[test]
fn eval_each_case_parse_success() {
    // 细粒度断言：每条用例单独可见，便于定位回归
    let reports = run_eval();
    for r in &reports {
        assert!(
            r.parse_ok,
            "用例 `{}` 解析失败（clean_and_parse 返回错误）",
            r.id
        );
        assert!(
            r.accuracy() >= 0.8,
            "用例 `{}` 字段准确率 {:.0}% 低于 80%",
            r.id,
            r.accuracy() * 100.0
        );
    }
}
