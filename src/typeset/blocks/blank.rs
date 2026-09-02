//! 答题留白：要不要留、留多高、用哪种样式（T4.4，§6.2）
//!
//! ## 三个来源，优先级在这一处定死
//!
//! 1. **逐题** `q.answer_space` —— 试题篮里给某道题单独设过；
//! 2. **全卷** `options.answer_space` —— 请求级开关（B5 说的「开关在 options 手里」）；
//! 3. **版面** `spec.answer_blank` —— 样式与默认高度，导出面板的「留白样式 / 高度」落在这里。
//!
//! 1、2 出现时样式以它为准（B5「冲突时以 options 为准」，卷级冲突由 `export::pdf` 记一条
//! info），只有两者都没表态才落到 3。
//!
//! **没有第 3 档，留白就永远不会出现**：wire 的 `AnswerSpace` 是整块 `Option`，而前端只写
//! `spec.answer_blank.style`、从不填 `options.answer_space` —— 于是「学生卷 = 题干 + 选项 + 留白」
//! 这条 T4.6 的默认口径在版面上根本立不起来，导出面板里那个留白下拉也是个死控件。版面侧高度
//! 非正数仍算「作者明确关掉了留白」，不做兜底。
//!
//! ## 教师（讲义）模式一律不留白
//!
//! 讲义上的作答区不是给学生写的，它在教师侧的正确形态是解析：考点 / 易错 / 点拨 / 思路四类
//! Callout 由装配器按 `options.callouts` 挂到题块上（[`crate::export::assembler`]），答案与
//! 全解全析按 `answer_at_end` 决定内嵌题末还是走卷末答案区（`export::pdf::question_blocks`）。
//! 所以 §6.2 那句「教师版折叠为解析 Callout」在排版侧只需要**不出留白** —— 再补一块就是把解析
//! 印两遍。

use crate::export::model::{AnswerSpace, BlankStyle as WireBlankStyle, ExamQuestion};
use crate::typeset::blocks::{BlockCtx, Policy, Registry};
use crate::typeset::ir::BlankBlock;
use crate::typeset::spec::{BlankStyle, LayoutSpec, OutputProfile, ResolvedBlank};

/// 本题的留白块，`None` = 这一题不留白
pub fn plan(q: &ExamQuestion, ctx: &BlockCtx, policy: &Policy) -> Option<BlankBlock> {
    if !policy.wants_blank || ctx.profile == OutputProfile::Teacher {
        return None;
    }
    let space = q.answer_space.or(ctx.options.answer_space);
    let resolved = match space {
        Some(space) => from_space(space, ctx.spec)?,
        None => from_spec(ctx.spec)?,
    };
    // 零高或负高的留白画出来是个不占地方的空块，不如干脆不留
    if resolved.height_mm <= 0.0 {
        return None;
    }
    Some(BlankBlock::new(q.number, &resolved))
}

/// 逐题 / 全卷给了：开关已经翻开，样式归它（B5），高度非正数时退回版面兜底值
fn from_space(space: AnswerSpace, spec: &LayoutSpec) -> Option<ResolvedBlank> {
    let mut resolved = spec.resolve_blank(Some(space.height_cm as f32))?;
    resolved.style = style_of(space.style);
    Some(resolved)
}

/// 谁都没表态：版面侧的样式与默认高度一起用；高度非正数 = 留白被关掉
fn from_spec(spec: &LayoutSpec) -> Option<ResolvedBlank> {
    if spec.answer_blank.height_cm <= 0.0 {
        return None;
    }
    spec.resolve_blank(Some(spec.answer_blank.height_cm))
}

/// 内容侧样式 → 版面侧样式（两套枚举各自服务各自的 wire 契约，在桥上相遇）
fn style_of(s: WireBlankStyle) -> BlankStyle {
    match s {
        WireBlankStyle::Lines => BlankStyle::Lines,
        WireBlankStyle::Dots => BlankStyle::Dots,
        WireBlankStyle::Blank => BlankStyle::Blank,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::model::{ExportOptions, QuestionKind};
    use crate::typeset::spec::LayoutSpec;

    fn space(style: WireBlankStyle, height_cm: f64) -> Option<AnswerSpace> {
        Some(AnswerSpace { style, height_cm })
    }

    fn question(answer_space: Option<AnswerSpace>) -> ExamQuestion {
        ExamQuestion {
            number: 18,
            score: 12.0,
            kind: QuestionKind::Solution,
            stem: Vec::new(),
            options: Vec::new(),
            answers: Vec::new(),
            analyses: Vec::new(),
            structure_parts: Vec::new(),
            callouts: Vec::new(),
            answer_space,
            issues: Vec::new(),
        }
    }

    /// 留白决策不看 registry，但 `BlockCtx` 带着它：给一个空的就够，省得每个用例再造
    fn ctx<'a>(
        options: &'a ExportOptions,
        spec: &'a LayoutSpec,
        registry: &'a Registry,
        profile: OutputProfile,
    ) -> BlockCtx<'a> {
        BlockCtx {
            options,
            spec,
            profile,
            available_em: 30.0,
            registry,
        }
    }

    fn plan_with(
        answer_space: Option<AnswerSpace>,
        options_space: Option<AnswerSpace>,
        spec: &LayoutSpec,
        profile: OutputProfile,
    ) -> Option<BlankBlock> {
        let registry = Registry::standard();
        let options = ExportOptions {
            answer_space: options_space,
            ..Default::default()
        };
        let ctx = ctx(&options, spec, &registry, profile);
        plan(&question(answer_space), &ctx, &WRITTEN)
    }

    const WRITTEN: Policy = Policy {
        expands_parts: true,
        wants_blank: true,
        compact_stem: false,
    };

    /// 只有题型模板要求留白的题才走到决策这一步
    #[test]
    fn policy_without_blank_asked_never_blanks() {
        let registry = Registry::standard();
        let options = ExportOptions {
            answer_space: space(WireBlankStyle::Lines, 4.0),
            ..Default::default()
        };
        let spec = LayoutSpec::default();
        let ctx = ctx(&options, &spec, &registry, OutputProfile::Student);
        let short = Policy {
            wants_blank: false,
            ..Default::default()
        };
        assert_eq!(plan(&question(None), &ctx, &short), None);
        // 开关与样式都齐了也不留：选择 / 填空的作答位在别处（选项栅格 / 行内下划线 B2）
        assert_eq!(
            plan(&question(space(WireBlankStyle::Dots, 9.0)), &ctx, &short),
            None
        );
    }

    /// 模式切换：学生与考卷留白，讲义一律不留（解析改走 Callout 与答案区）
    #[test]
    fn lecture_profile_never_blanks_but_student_and_exam_do() {
        let spec = LayoutSpec::default();
        for profile in [OutputProfile::Student, OutputProfile::Exam] {
            let blank = plan_with(None, None, &spec, profile);
            assert!(blank.is_some(), "{profile:?} 该有留白");
        }
        assert_eq!(
            plan_with(None, None, &spec, OutputProfile::Teacher),
            None,
            "讲义不留白"
        );
        // 逐题显式要求也照样不留 —— 模式是权威，不是建议
        assert_eq!(
            plan_with(
                space(WireBlankStyle::Lines, 6.0),
                None,
                &spec,
                OutputProfile::Teacher
            ),
            None
        );
    }

    /// 三级优先级的完整排序
    #[test]
    fn question_space_beats_paper_options_beats_layout_default() {
        let spec = LayoutSpec::default();
        let default_height = spec.answer_blank.height_cm * 10.0;

        // 谁都没表态 → 版面侧的样式与高度
        let only_spec = plan_with(None, None, &spec, OutputProfile::Student).unwrap();
        assert_eq!(only_spec.style, spec.answer_blank.style);
        assert_eq!(only_spec.height_mm, default_height);

        // 只有全卷 options 表态 → 样式与高度都归它（哪怕与 spec 不同，B5）
        let paper = plan_with(
            None,
            space(WireBlankStyle::Dots, 3.0),
            &spec,
            OutputProfile::Student,
        )
        .unwrap();
        assert_eq!(paper.style, BlankStyle::Dots);
        assert_eq!(paper.height_mm, 30.0);

        // 逐题再盖一层：高度归题，样式也归题
        let per_question = plan_with(
            space(WireBlankStyle::Blank, 2.0),
            space(WireBlankStyle::Dots, 3.0),
            &spec,
            OutputProfile::Exam,
        )
        .unwrap();
        assert_eq!(per_question.style, BlankStyle::Blank);
        assert_eq!(per_question.height_mm, 20.0);
        assert_eq!(per_question.number, 18, "留白要挂在它自己的题号上");
    }

    /// options 只否定了高度（0 / 负数）时退回版面兜底，而不是整块取消
    #[test]
    fn non_positive_height_falls_back_to_the_layout_default() {
        let spec = LayoutSpec::default();
        let fallback = spec.answer_blank.height_cm * 10.0;
        for cm in [0.0, -2.0] {
            let got = plan_with(
                space(WireBlankStyle::Lines, cm),
                None,
                &spec,
                OutputProfile::Student,
            )
            .unwrap_or_else(|| panic!("{cm}cm 该退回兜底高度"));
            assert_eq!(got.height_mm, fallback, "{cm}cm");
            // 退的是高度，不是样式：options 表过态就还按它的样式
            assert_eq!(got.style, BlankStyle::Lines, "{cm}cm");
        }
    }

    /// 版面侧高度非正数 = 作者把留白关了，此时不再兜底
    #[test]
    fn a_layout_that_forbids_blanks_stays_blankless() {
        let mut spec = LayoutSpec::default();
        spec.answer_blank.height_cm = 0.0;
        assert_eq!(
            plan_with(None, None, &spec, OutputProfile::Student),
            None,
            "spec 关了留白，逐题没提就不该出现"
        );
        // 但显式要了高度的题不受影响：开关在 options 手里
        let asked = plan_with(
            space(WireBlankStyle::Lines, 5.0),
            None,
            &spec,
            OutputProfile::Student,
        )
        .expect("逐题要了留白就该有");
        assert_eq!(asked.height_mm, 50.0);
    }

    /// 三套样式都要能落到 IR 上 —— 版面上的差异由渲染器负责，这里只管传对
    #[test]
    fn every_wire_style_reaches_the_ir() {
        let spec = LayoutSpec::default();
        let pairs = [
            (WireBlankStyle::Lines, BlankStyle::Lines),
            (WireBlankStyle::Dots, BlankStyle::Dots),
            (WireBlankStyle::Blank, BlankStyle::Blank),
        ];
        for (wire, expected) in pairs {
            let got = plan_with(space(wire, 4.0), None, &spec, OutputProfile::Student).unwrap();
            assert_eq!(got.style, expected, "{wire:?} 传丢了");
        }
    }
}
