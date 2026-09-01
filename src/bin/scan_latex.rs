//! T2.1 ⛔ latex2mathml 覆盖率预扫描（实施计划 §15.3 P1）
//!
//! 只读工具：把全库题目文本切成公式，逐条跑 `latex2mathml`，统计降级率。
//! 出口条件：语料降级率 ≤5% 通过门；>5% 停下评审备选方案（KaTeX `output:'mathml'`
//! 预转换 / temml），不得带病推进。
//!
//! 两道信号分开看：
//!   1. 真实语料（DB）——决定门是否通过，但受本地库规模限制，样本小则结论弱
//!   2. 教辅高频环境探针集（内置，不入库）——即使语料干净，`\begin{cases}` 这类
//!      高频结构失败仍是 M2 的现实风险，故单列 WARN 不参与门判定
//!
//! 运行方式：
//!   cargo run --bin scan_latex            # 全库 + 探针集
//!   cargo run --bin scan_latex -- --limit 500 --samples 20
//!
//! 退出码：0 = 门下通过；2 = 门未通过（需评审备选方案）；1 = 运行错误

use std::collections::BTreeMap;

use serde_json::Value;

use mathset::config::AppConfig;
use mathset::db;
use mathset::export::content::split_content;
use mathset::export::model::InlineNode;

#[derive(Debug, sqlx::FromRow)]
struct QuestionRow {
    id: uuid::Uuid,
    stem: String,
    options: Option<Value>,
    correct_answer: Value,
    analysis: Option<String>,
    structure: Option<Value>,
}

/// 一条公式的转换结果
#[derive(Default, Debug)]
struct Stats {
    total: usize,
    failed: usize,
    /// 失败公式原文 → 计数 + 原因 + 复现样例
    failures: BTreeMap<String, Failure>,
    /// 失败公式中的宏命令 → 出现次数
    commands: BTreeMap<String, usize>,
    /// 字段（stem / analysis / options / …）→ (公式总数, 失败数)
    per_field: BTreeMap<&'static str, (usize, usize)>,
    /// 涉及失败的题目 id
    failed_questions: usize,
}

/// 一条失败公式的聚合信息
#[derive(Debug)]
struct Failure {
    count: usize,
    reason: String,
    /// 「字段 @ 题目 id」，便于直接去库里复现
    example: String,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let limit = arg_usize(&args, "--limit").unwrap_or(u32::MAX);
    let samples = arg_usize(&args, "--samples").unwrap_or(20usize as u32) as usize;

    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(run(limit, samples));
}

async fn run(limit: u32, samples: usize) {
    let _ = dotenvy::dotenv();
    let config = AppConfig::from_env();
    let pool = db::create_pool(&config.database_url, 4).await;

    let rows: Vec<QuestionRow> = sqlx::query_as(
        "SELECT id, stem, options, correct_answer, analysis, structure \
          FROM questions ORDER BY created_at LIMIT $1",
    )
    .bind(limit as i64)
    .fetch_all(&pool)
    .await
    .expect("读取题目失败");

    let mut stats = Stats::default();
    for row in &rows {
        let mut failed_here = false;
        for (field, text) in question_texts(row) {
            for latex in math_in(&text) {
                let counter = stats.per_field.entry(field).or_insert((0, 0));
                counter.0 += 1;
                stats.total += 1;
                if let Err(reason) = convert(&latex) {
                    stats.failed += 1;
                    counter.1 += 1;
                    failed_here = true;
                    let e = stats
                        .failures
                        .entry(latex.clone())
                        .or_insert_with(|| Failure {
                            count: 0,
                            reason,
                            example: format!("{field} @ {}", row.id),
                        });
                    e.count += 1;
                    for cmd in commands_in(&latex) {
                        *stats.commands.entry(cmd).or_insert(0) += 1;
                    }
                }
            }
        }
        if failed_here {
            stats.failed_questions += 1;
        }
    }

    println_report(&rows, &stats, samples);
    let probe_failed = println_probe();

    let rate = if stats.total > 0 {
        stats.failed as f64 * 100.0 / stats.total as f64
    } else {
        0.0
    };
    let gate_ok = rate <= 5.0;
    println!("━━ 决策门（T2.1 ⛔，出口条件：语料降级率 ≤5%）━━");
    println!(
        "语料降级率 {rate:.2}% → {}",
        if gate_ok {
            "通过"
        } else {
            "未通过：停下评审备选方案（KaTeX output:'mathml' 预转换 / temml）"
        }
    );
    if probe_failed > 0 && gate_ok {
        println!("⚠ 探针集有 {probe_failed} 条高频环境失败，门虽通过仍需在 M2 排期里记为已知边界");
    }
    std::process::exit(if gate_ok { 0 } else { 2 });
}

/// 一道题里所有可能含公式的文本（题干 / 解析 / 选项 / 标准答案 / 问树）
fn question_texts(row: &QuestionRow) -> Vec<(&'static str, String)> {
    let mut out = vec![("stem", row.stem.clone())];
    if let Some(a) = &row.analysis {
        out.push(("analysis", a.clone()));
    }
    for (label, value) in [
        ("options", &row.options),
        ("correct_answer", &Some(row.correct_answer.clone())),
        ("structure", &row.structure),
    ] {
        let mut buf = String::new();
        collect_strings(value.as_ref(), &mut buf);
        if !buf.is_empty() {
            out.push((label, buf));
        }
    }
    out
}

/// JSONB 里的所有字符串叶子（换行拼接，交给切分器统一处理）
fn collect_strings(v: Option<&Value>, buf: &mut String) {
    let Some(v) = v else { return };
    match v {
        Value::String(s) => {
            buf.push_str(s);
            buf.push('\n');
        }
        Value::Array(items) => {
            for item in items {
                collect_strings(Some(item), buf);
            }
        }
        Value::Object(map) => {
            for item in map.values() {
                collect_strings(Some(item), buf);
            }
        }
        _ => {}
    }
}

/// 文本 → 公式源串列表（表格单元格二次切分）
fn math_in(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    collect_math(&split_content(text), &mut out);
    out
}

fn collect_math(nodes: &[InlineNode], out: &mut Vec<String>) {
    for node in nodes {
        match node {
            InlineNode::Math { latex, .. } => out.push(latex.clone()),
            InlineNode::Table { header, rows, .. } => {
                for cell in header.iter().chain(rows.iter().flatten()) {
                    collect_math(&split_content(cell), out);
                }
            }
            InlineNode::ImgRow { caption, .. } => {
                if let Some(c) = caption {
                    collect_math(&split_content(c), out);
                }
            }
            _ => {}
        }
    }
}

fn convert(latex: &str) -> Result<(), String> {
    match latex2mathml::latex_to_mathml(latex, latex2mathml::DisplayStyle::Block) {
        Ok(mathml) if mathml.contains("<math") => Ok(()),
        Ok(_) => Err("输出为空（未产出 mathml）".to_string()),
        Err(e) => Err(e.to_string()),
    }
}

fn commands_in(latex: &str) -> Vec<String> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| regex::Regex::new(r"\\([A-Za-z]+)").unwrap());
    re.captures_iter(latex)
        .map(|c| format!("\\{}", &c[1]))
        .collect()
}

fn println_report(rows: &[QuestionRow], stats: &Stats, samples: usize) {
    let rate = if stats.total > 0 {
        stats.failed as f64 * 100.0 / stats.total as f64
    } else {
        0.0
    };
    println!("━━ 真实语料扫描 ━━");
    println!(
        "题目 {} 道 / 公式 {} 条 / 失败 {} 条（{rate:.2}%）/ 受影响题目 {} 道",
        rows.len(),
        stats.total,
        stats.failed,
        stats.failed_questions
    );
    if stats.failures.is_empty() {
        println!("无失败公式");
        return;
    }
    println!("— 按字段统计 —");
    for (field, (total, failed)) in &stats.per_field {
        println!(
            "  {field:<15} 公式 {total:>4} / 失败 {failed:>3}{}",
            if *failed > 0 {
                format!(" ({:.1}%)", *failed as f64 * 100.0 / *total as f64)
            } else {
                String::new()
            }
        );
    }
    println!("— 失败样例（最多 {samples} 条，含原因与复现位置）—");
    let mut listed: Vec<(&String, &Failure)> = stats.failures.iter().collect();
    listed.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(&b.0)));
    for (latex, f) in listed.iter().take(samples) {
        println!(
            "  [{:>3}×] {latex}\n         ↳ {}\n         位置: {}",
            f.count, f.reason, f.example
        );
    }
    println!("— 失败公式中的宏命令频次 —");
    let mut cmds: Vec<(&String, &usize)> = stats.commands.iter().collect();
    cmds.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (cmd, n) in cmds.iter().take(20) {
        println!("  {n:>3}× {cmd}");
    }
}

/// 教辅高频 LaTeX 环境探针集（口径：人教版 / 高考常见写法）
fn println_probe() -> usize {
    const PROBES: &[(&str, &str)] = &[
        ("分式", r"\frac{1}{x+1}"),
        ("根号", r"\sqrt{2}+\sqrt[3]{a}"),
        (
            "分段函数 cases",
            r"f(x)=\begin{cases}x^2,&x\ge 0\\-x,&x<0\end{cases}",
        ),
        ("aligned 多行", r"\begin{aligned}a&=1\\b&=2\end{aligned}"),
        ("gathered", r"\begin{gathered}x=1\\y=2\end{gathered}"),
        (
            "array 方程组",
            r"\left\{\begin{array}{l}x+y=1\\x-y=3\end{array}\right.",
        ),
        ("matrix", r"\begin{pmatrix}1&2\\3&4\end{pmatrix}"),
        ("求和上下限", r"\sum_{i=1}^{n}a_i"),
        ("积分", r"\int_0^1 x^2\,\mathrm{d}x"),
        ("极限", r"\lim_{x\to 0}\frac{\sin x}{x}=1"),
        (
            "导数",
            r"f'(x_0)=\lim_{\Delta x\to 0}\frac{f(x_0+\Delta x)-f(x_0)}{\Delta x}",
        ),
        (
            "三角",
            r"\sin^2 x+\cos^2 x=1,\quad \tan x=\frac{\sin x}{\cos x}",
        ),
        ("集合", r"A\cap B=\varnothing,\ A\subsetneq \mathbb{R}"),
        ("空集宏 emptyset", r"A=\emptyset"),
        ("对数指数", r"\log_2 8=3,\quad e^{i\pi}+1=0"),
        ("不等式", r"a^2+b^2\ge 2ab,\ |x-1|<3"),
        ("向量", r"\vec{a}\cdot\vec{b}=|\vec{a}||\vec{b}|\cos\theta"),
        ("立体几何角", r"\angle ABC=60^\circ,\ \triangle ABC"),
        ("排列组合", r"C_n^m=\frac{n!}{m!(n-m)!},\ A_n^m"),
        ("概率", r"P(A\mid B)=\frac{P(AB)}{P(B)}"),
        (
            "统计",
            r"\bar{x}=\frac{1}{n}\sum_{i=1}^{n}x_i,\quad s^2=\frac{1}{n}\sum(x_i-\bar{x})^2",
        ),
        ("二项式", r"(a+b)^n=\sum_{k=0}^{n}\binom{n}{k}a^{n-k}b^k"),
        ("数列", r"a_n=a_1+(n-1)d,\ S_n=\frac{n(a_1+a_n)}{2}"),
        ("圆锥曲线", r"\frac{x^2}{a^2}+\frac{y^2}{b^2}=1\ (a>b>0)"),
        ("上下标嵌套", r"x_i^2,\quad {}^1_2H,\quad a^{b^{c}}"),
        ("重音", r"\hat{a},\ \tilde{b},\ \bar{c},\ \vec{d},\ \dot{e}"),
        (
            "横线框",
            r"\overline{AB},\ \underline{a},\ \widehat{CD},\ \overrightarrow{AB}",
        ),
        (
            "括号尺寸",
            r"\left(\frac{x}{y}\right)^{2},\ \left[1,+\infty\right)",
        ),
        (
            " cases 内含分数",
            r"y=\begin{cases}\dfrac{1}{x},&x>0\\0,&x=0\end{cases}",
        ),
        ("text 中文", r"\text{当 } x>0 \text{ 时}"),
        ("mathrm 单位", r"v=3\,\mathrm{m/s}"),
        ("省略号", r"a_1,a_2,\cdots,a_n"),
        ("逻辑符号", r"p\Rightarrow q,\ p\Leftrightarrow \neg q"),
        ("反斜杠绝对值", r"\left|-\dfrac{1}{2}\right|=\dfrac{1}{2}"),
        ("组合宏", r"\binom{2025}{2}"),
        ("斜体分数堆叠", r"{1\over 2}"),
        // —— 环境改写可行性探测（决定 T2.2 归一层的方案）——
        ("matrix 含 & 对齐", r"\begin{matrix}a&=1\\b&=2\end{matrix}"),
        (
            "matrix 含 text 条件",
            r"\begin{matrix}x^2,&x\ge 0\\-x,&x<0\end{matrix}",
        ),
        ("align 环境", r"\begin{align}a&=1\\b&=2\end{align}"),
        ("align* 环境", r"\begin{align*}a&=1\\b&=2\end{align*}"),
        ("split 环境", r"\begin{split}a&=1\\b&=2\end{split}"),
        ("gather 环境", r"\begin{gather}a=1\\b=2\end{gather}"),
        ("dcases 环境", r"\begin{dcases}x^2,&x\ge 0\end{dcases}"),
        ("rcases 环境", r"\begin{rcases}x=1\\y=2\end{rcases}"),
        ("eqnarray 环境", r"\begin{eqnarray}a&=1\\b&=2\end{eqnarray}"),
        ("array 带 p 列", r"\begin{array}{p{2cm}}x\\y\end{array}"),
        ("subarray", r"\begin{subarray}{l}x=1\\y=2\end{subarray}"),
        ("matrix 单行", r"\begin{matrix}1&2\end{matrix}"),
        // —— 归一改写后必须产出的定界符形式（cases → {\matrix.）——
        (
            "左花括号+空右定界",
            r"f(x)=\left\{\begin{matrix}x^2,&x\ge 0\\-x,&x<0\end{matrix}\right.",
        ),
        (
            "空左定界+右花括号",
            r"\left.\begin{matrix}x=1\\y=2\end{matrix}\right\}",
        ),
        (
            "已有左花括号包 array→matrix",
            r"\left\{\begin{matrix}12-6a=0\\12a-24=0\end{matrix}\right.",
        ),
        ("非法输入（预期失败）", r"\frac{1}{"),
    ];
    println!("━━ 教辅高频环境探针集（内置，不参与门判定）━━");
    let mut failed = 0usize;
    for (label, latex) in PROBES {
        let r = convert(latex);
        match &r {
            Ok(()) => println!("  ✅ {label}"),
            Err(reason) => {
                failed += 1;
                println!("  ❌ {label}  {latex}\n         ↳ {reason}");
            }
        }
    }
    println!("探针：{} 条 / 失败 {failed} 条", PROBES.len());
    failed
}

fn arg_usize(args: &[String], flag: &str) -> Option<u32> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .and_then(|v| v.parse().ok())
}
