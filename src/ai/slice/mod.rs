//! MinerU Markdown 切片 / 清洗：从 OCR 原文切到规范 `ParsedQuestion`。
//!
//! 迭代方式（失败用例喂回后再改本模块，不把正则堆进 worker）：
//! 1. 收集边缘格式 MinerU 片段（见本目录测试夹具）
//! 2. 按规律写专用清洗 / 切片脚本
//! 3. 单元测试跑夹具，标出漏切、错切
//! 4. 把失败用例加进测试再改脚本
//!
//! 切片脚本测试阶段默认跳过异步打标：`MATHSET_ENABLE_TAGGING=1` 恢复。

mod clean;
mod parts;

pub use clean::clean_analysis_text;
pub use parts::peel_solution_sub_stems;

use crate::ai::types::ParsedQuestion;

/// 是否暂停解析后的异步打标（切片脚本迭代期默认暂停）。
pub fn tagging_paused() -> bool {
    !matches!(std::env::var("MATHSET_ENABLE_TAGGING").as_deref(), Ok("1"))
}

/// 对已切块的题目做一次清洗：去【分析】/【点睛】、解答题小问题干入 parts。
pub fn polish_question(q: &mut ParsedQuestion) {
    clean::clean_question_editorial(q);
    parts::peel_solution_sub_stems(q);
    clean::strip_empty_analysis(q);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::structure::{recover_question_sections, structure_chunk};
    use crate::ai::types::ParsedAnswer;

    fn from_mineru(md: &str) -> crate::ai::types::ParsedQuestion {
        let mut q = structure_chunk(md).question;
        recover_question_sections(&mut q, md);
        q
    }

    fn choice_letters(q: &crate::ai::types::ParsedQuestion) -> Vec<String> {
        match &q.correct_answer {
            Some(ParsedAnswer::Choice { options }) => options.clone(),
            _ => vec![],
        }
    }

    /// 图1：选择题 【答案】$\mathrm{B}$，解析里的【分析】【详解】应清掉标签与思路摘要。
    #[test]
    fn fixture_choice_mathrm_answer_strips_fenxi() {
        let md = "\
8. 已知向量 $\\vec{a}=(0,1),\\vec{b}=(2,x)$，若 $\\vec{b}\\perp(\\vec{b}-4\\vec{a})$，则 $x=(\\quad)$\n\
A. $-2$\n\
B. $-1$\n\
C. $1$\n\
D. $2$\n\
【答案】$\\mathrm{B}$\n\
【解析】\n\
【分析】根据向量垂直的坐标运算可求 $x$ 的值。\n\
【详解】因为 $\\vec{b}\\perp(\\vec{b}-4\\vec{a})$，所以 $\\vec{b}\\cdot(\\vec{b}-4\\vec{a})=0$。故 $x=2$。\n";
        let q = from_mineru(md);
        assert_eq!(q.question_type, "choice");
        assert_eq!(choice_letters(&q), vec!["B".to_string()], "{:?}", q.correct_answer);
        let blob: String = q.analysis.iter().map(|a| a.content.clone()).collect();
        assert!(!blob.contains("【分析】"), "{blob}");
        assert!(!blob.contains("【详解】"), "{blob}");
        assert!(!blob.contains("根据向量垂直"), "思路摘要不应留在解析: {blob}");
        assert!(blob.contains("因为"), "{blob}");
        assert!(!q.stem.contains("A."), "选项应离开题干: {}", q.stem);
    }

    /// 图2：【点睛】不得进入解析。
    #[test]
    fn fixture_choice_strips_dianjing() {
        let md = "\
4. 已知函数 $f(1)=1$。则 $f(11)=(\\quad)$\n\
A. $1$\n\
B. $2$\n\
C. $3$\n\
D. $4$\n\
【答案】B\n\
【解析】\n\
【分析】由递推即可求解。\n\
【详解】$f(1)=1$，故选：B。\n\
【点睛】关键点点睛：递推放缩是常用技巧，注意奇偶性。\n";
        let q = from_mineru(md);
        assert_eq!(choice_letters(&q), vec!["B".to_string()]);
        let blob: String = q.analysis.iter().map(|a| a.content.clone()).collect();
        assert!(!blob.contains("【点睛】"), "{blob}");
        assert!(!blob.contains("关键点点睛"), "{blob}");
        assert!(!blob.contains("【分析】"), "{blob}");
        assert!(blob.contains("f(1)"), "{blob}");
    }

    /// 图3/图4：解答题（1）（2）应进入本问题干，而不是只堆在总题干里留下空叶子。
    #[test]
    fn fixture_solution_sub_stems_fill_parts() {
        let md = "\
15. 记 $\\triangle ABC$ 内角 $A,B,C$ 的对边分别为 $a,b,c$，已知 $\\sin C=\\sqrt{2}\\cos B$，$a^2+b^2-c^2=\\sqrt{2}ab$。\n\
（1）求 $B$；\n\
（2）若 $\\triangle ABC$ 的面积为 $3+\\sqrt{3}$，求 $c$。\n\
【答案】（1）$B=\\dfrac{\\pi}{3}$（2）$2\\sqrt{2}$\n\
【解析】\n\
【详解】由余弦定理得 $\\cos C=\\dfrac{\\sqrt{2}}{2}$。\n";
        let q = from_mineru(md);
        assert_eq!(q.question_type, "solution");
        assert!(
            !q.stem.contains("（1）") && !q.stem.contains("（2）"),
            "小问应从总题干剥离: {}",
            q.stem
        );
        assert!(q.stem.contains("\\triangle ABC"), "{}", q.stem);
        assert!(q.parts.len() >= 2, "parts={:?}", q.parts.iter().map(|p| &p.label).collect::<Vec<_>>());
        let stems: Vec<String> = q.parts.iter().map(|p| p.stem.clone()).collect();
        assert!(
            stems.iter().any(|s| s.contains("求") && s.contains("B")),
            "小问1题干应回填: {stems:?}"
        );
        assert!(
            stems.iter().any(|s| s.contains("面积") || s.contains("求 $c") || s.contains("求 c")),
            "小问2题干应回填: {stems:?}"
        );
        assert!(stems.iter().all(|s| !s.trim().is_empty()), "不得留下空的本问题干: {stems:?}");
    }

    /// 图5：解析里混入的【答案】【解析】【分析】应清掉，答案进 answer 字段。
    #[test]
    fn fixture_solution_peels_answer_out_of_analysis() {
        let md = "\
16. 已知椭圆 $C:\\dfrac{x^2}{a^2}+\\dfrac{y^2}{b^2}=1(a>b>0)$。\n\
（1）求离心率；\n\
【答案】（1）$\\dfrac{1}{2}$\n\
【解析】\n\
【分析】代入两点得到关于 $a,b$ 的方程，解出即可。\n\
【详解】由 $a=2$ 得 $e=\\dfrac{1}{2}$。\n";
        let q = from_mineru(md);
        let blob: String = q
            .analysis
            .iter()
            .map(|a| a.content.clone())
            .chain(q.parts.iter().flat_map(|p| p.analyses.iter().map(|a| a.content.clone())))
            .collect();
        assert!(!blob.contains("【答案】"), "{blob}");
        assert!(!blob.contains("【解析】"), "{blob}");
        assert!(!blob.contains("【分析】"), "{blob}");
        assert!(!blob.contains("解出即可"), "分析提纲不应留在解析: {blob}");
        let answers: Vec<String> = q
            .parts
            .iter()
            .filter_map(|p| p.answer.clone())
            .filter(|a| !a.trim().is_empty())
            .collect();
        assert!(
            answers.iter().any(|a| a.contains("dfrac") || a.contains("1}{2") || a.contains("1/2")),
            "答案应回填到问树: {answers:?} correct={:?}",
            q.correct_answer
        );
    }

    #[test]
    fn clean_analysis_is_idempotent() {
        let raw = "【分析】由题意即可。\n【详解】因为 $x=1$，所以选 A。\n【点睛】注意定义域。";
        let once = clean_analysis_text(raw);
        let twice = clean_analysis_text(&once);
        assert_eq!(once, twice);
        assert!(!once.contains("【"));
        assert!(!once.contains("由题意即可"));
        assert!(once.contains("因为"));
    }

    /// 杭州二中样卷图：【分析】与演算只隔一个换行，「判断即可」也要丢掉。
    #[test]
    fn fixture_hangzhou_choice_drops_strategy_first_line() {
        let md = "\
1. 命题“ $\\exists x > 0, x^2 + 2x + 3 = 0$ ”的否定是（）\n\
A. $\\forall x \\leq 0, x^2 + 2x + 3 = 0$\n\
B. $\\forall x > 0, x^2 + 2x + 3 \\neq 0$\n\
C. $\\exists x \\leq 0, x^2 + 2x + 3 = 0$\n\
D. $\\exists x > 0, x^2 + 2x + 3 \\neq 0$\n\
故选：B\n\
【解析】\n\
【分析】根据存在量词命题的否定为全称量词命题判断即可.\n\
【详解】命题“ $\\exists x>0,x^{2}+2x+3=0$ ”为存在量词命题，其否定为全称量词命题。故选：B\n";
        let q = from_mineru(md);
        let blob: String = q.analysis.iter().map(|a| a.content.clone()).collect();
        assert!(!blob.contains("判断即可"), "短思路不应留在解析: {blob}");
        assert!(!blob.contains("根据存在量词"), "{blob}");
        assert!(blob.contains("存在量词命题") || blob.contains("故选"), "{blob}");
        assert!(q.analysis.iter().all(|a| a.title != "分析" && a.title != "详解"));
    }

    /// 杭州二中第 15 题：无总前提、行首就是（1）（2）。
    #[test]
    fn fixture_hangzhou_q15_peels_leading_subqs_and_clears_analysis() {
        let md = "\
15. （1）求值： $\\left(3\\frac{3}{8}\\right)^{-\\frac{2}{3}}$ ；\n\
（2）若 $a + a^{-1} = 3$ ，求 $\\frac{9}{4}$\n\
【答案】（1） $-5\\sqrt{5}$ ；（2） $\\frac{9}{4}$\n\
【解析】\n\
【详解】（1）原式 $= -5\\sqrt{5}$\n\
（2）平方得 $\\frac{9}{4}$\n";
        let q = from_mineru(md);
        assert_eq!(q.question_type, "solution");
        assert!(
            !q.stem.contains("（1）") && !q.stem.contains("（2）"),
            "无总前提时小问不得留在 stem: {}",
            q.stem
        );
        assert!(q.parts.len() >= 2, "parts={:?}", q.parts.iter().map(|p| &p.label).collect::<Vec<_>>());
        assert!(
            q.analysis.is_empty(),
            "解答题整题 analysis 应为空: {:?}",
            q.analysis
        );
    }

    /// 杭州二中第 16 题：parts 已有解析时，丢掉整题 analysis 副本。
    #[test]
    fn fixture_hangzhou_q16_sinks_question_analysis() {
        let md = "\
16. 已知全集为 $\\mathbf{R}$ ，集合 $A = \\{x|0 < x < 2\\}$ ， $B = \\{x|1\\leq x\\leq 3\\}$ 。\n\
（1）求 $A \\cup B$ ；\n\
（2）若 $A \\cup C = A$ ，求实数 $t$ 的取值范围。\n\
【答案】（1） $\\{x \\mid 0 < x \\leq 3\\}$ （2） $[0, +\\infty)$\n\
【解析】\n\
（1） $A \\cup B = \\{x \\mid 0 < x \\leq 3\\}$\n\
(2) $[0, +\\infty)$\n\
【详解】由 $1 < 2^{x} < 4$ 得 $A$。故 $A \\cup B = \\{x \\mid 0 < x \\leq 3\\}$ 。\n";
        let q = from_mineru(md);
        assert_eq!(q.question_type, "solution");
        assert!(q.parts.len() >= 2, "parts len={}", q.parts.len());
        assert!(
            q.analysis.is_empty(),
            "解答题整题 analysis 应为空: {:?}",
            q.analysis
        );
        let leaf_blob: String = q
            .parts
            .iter()
            .flat_map(|p| p.analyses.iter().map(|a| a.content.clone()))
            .collect();
        assert!(
            leaf_blob.contains("A") || q.parts.iter().any(|p| p.answer.as_ref().is_some_and(|a| !a.is_empty())),
            "叶子应有解析或答案: parts={:?}",
            q.parts
        );
    }
}
