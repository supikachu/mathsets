//! 版面参数模型（实施计划 §6.1）— 任务 T3.2
//!
//! `LayoutSpec` 是排版域（模块 B）的唯一入参：`/export/pdf` 按 mode 取默认 spec，请求里的
//! `spec` 字段做字段级覆盖。经 ts-rs 导出 `frontend/src/api/types/layout.ts`（B6），
//! 前端 PDF 展开区的字段全部由它生成，不手写。
//!
//! 依赖方向：typeset **不** import export。`ExportMode → OutputProfile` 的翻译与
//! `ExamQuestion.answer_space` 的取值都在适配器 `export::pdf` 里做；本模块只交出合并规则
//! （B5：options 决定开关与高度，spec 决定样式）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 纸张：M3 基础版只用 `A4`；两档 A3 的字段先定齐，对折与三栏的版式在 T4.7/T4.8 落地
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub enum Paper {
    #[default]
    A4,
    /// A3 对折（8 开）：一张 A3 横向对折成两页 A4
    A3Fold,
    /// A3 三栏
    A3Tri,
}

impl Paper {
    /// 物理纸面宽 × 高（mm）。A3 两档都按长边横置出纸 —— 对折与三栏都要靠长边分栏
    pub fn size_mm(self) -> (u32, u32) {
        match self {
            Self::A4 => (210, 297),
            Self::A3Fold | Self::A3Tri => (420, 297),
        }
    }
}

/// 四边边距 + 栏间距（mm）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct Margins {
    pub top_mm: f32,
    pub right_mm: f32,
    pub bottom_mm: f32,
    pub left_mm: f32,
    /// 栏间距；单栏时由 [`LayoutSpec::column_gutter_mm`] 归零
    pub gutter_mm: f32,
}

impl Default for Margins {
    fn default() -> Self {
        Self {
            top_mm: 22.0,
            right_mm: 18.0,
            bottom_mm: 22.0,
            left_mm: 18.0,
            gutter_mm: 0.0,
        }
    }
}

/// 装订位
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub enum BindingPosition {
    /// A4 左侧 / A3 三栏的左侧密封线
    #[default]
    Left,
    /// A3 对折中线
    CenterFold,
}

/// 密封线旁的填涂区开关
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct BindingAreas {
    pub school: bool,
    /// 班级（`class` 是 Rust 关键字，序列化出去仍叫 class）
    #[serde(rename = "class")]
    pub class_name: bool,
    pub name: bool,
    pub exam_no: bool,
}

/// 密封线（`None` = 不装订）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct Binding {
    pub position: BindingPosition,
    pub areas: BindingAreas,
}

/// 页眉页脚开关。动态页眉取当前大题名（T4.10）与按逻辑页判定的奇偶外侧对齐（T4.7）尚未实现，
/// 字段先按 §6.1 定齐，免得 M4 再改前端 TS 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct HeaderFooter {
    /// 页眉：当前大题名
    pub header_title: bool,
    /// 页脚：第 X 页 共 Y 页
    pub page_number: bool,
    /// 奇偶页码外侧对齐（双面印）
    pub odd_even_outer: bool,
}

impl Default for HeaderFooter {
    fn default() -> Self {
        Self {
            header_title: false,
            page_number: true,
            odd_even_outer: false,
        }
    }
}

/// 出片对象。与导出域的 `ExportMode` 同形但独立 —— typeset 不依赖 export
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub enum OutputProfile {
    #[default]
    Student,
    Teacher,
    Exam,
}

/// 字体族名（typst `set text(font: ...)` 直接吃）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct FontSpec {
    /// 正文：思源宋体
    pub body: String,
    /// 标题：思源黑体
    pub heading: String,
    /// 数学：typst 内嵌的 New Computer Modern
    pub math: String,
}

impl Default for FontSpec {
    fn default() -> Self {
        Self {
            body: "Source Han Serif SC".into(),
            heading: "Source Han Sans SC".into(),
            math: "New Computer Modern".into(),
        }
    }
}

/// 答题留白样式
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub enum BlankStyle {
    /// 横线格
    #[default]
    Lines,
    /// 点阵
    Dots,
    /// 纯空白
    Blank,
}

/// 留白样式与默认高度。**是否留白与最终高度由 `options.answer_space` 裁决**（B5）
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct BlankSpec {
    pub style: BlankStyle,
    /// options 没给高度时的兜底高度（cm）
    pub height_cm: f32,
}

impl Default for BlankSpec {
    fn default() -> Self {
        Self {
            style: BlankStyle::Lines,
            height_cm: 6.0,
        }
    }
}

/// 印前色彩模式
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub enum ColorMode {
    #[default]
    Rich,
    /// K100：全文纯黑，防四色黑重影（T4.12）
    PrintBlackOnly,
}

/// 版面参数（§6.1 全字段）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct LayoutSpec {
    pub paper: Paper,
    /// 1 | 2 | 3
    pub columns: u8,
    pub margins: Margins,
    pub binding: Option<Binding>,
    pub header_footer: HeaderFooter,
    pub profile: OutputProfile,
    pub fonts: FontSpec,
    pub answer_blank: BlankSpec,
    pub color: ColorMode,
}

impl Default for LayoutSpec {
    fn default() -> Self {
        Self {
            paper: Paper::A4,
            columns: 1,
            margins: Margins::default(),
            binding: None,
            header_footer: HeaderFooter::default(),
            profile: OutputProfile::default(),
            fonts: FontSpec::default(),
            answer_blank: BlankSpec::default(),
            color: ColorMode::default(),
        }
    }
}

/// B5 合并后的留白：排版器只认这一份
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct ResolvedBlank {
    pub style: BlankStyle,
    pub height_mm: f32,
}

impl LayoutSpec {
    /// 单栏可用版心宽（mm）= 纸宽 − 左边距 − 右边距 − (n−1) × 栏距，再除以 n
    ///
    /// docx 侧另有一套 twips 口径（T2.7），两边都只有一处实现，漂了用例会先炸
    pub fn column_width_mm(&self) -> f32 {
        let (w, _) = self.paper.size_mm();
        let n = self.columns.max(1) as f32;
        let gutters = (n - 1.0) * self.margins.gutter_mm;
        ((w as f32) - self.margins.left_mm - self.margins.right_mm - gutters) / n
    }

    /// 生效的栏间距：单栏恒为 0，避免母版里写出没有意义的 `column-gutter`
    pub fn column_gutter_mm(&self) -> f32 {
        if self.columns > 1 {
            self.margins.gutter_mm
        } else {
            0.0
        }
    }

    /// 留白合并（B5）：**开关在 options 手里** —— `None` 表示这一题不留白，`spec.answer_blank`
    /// 完全不参与；`Some(height)` 表示留白，高度优先取 options，非正数视为「没填高度」退回
    /// spec 的兜底值，样式恒取 spec（options 侧的样式字段不参与）。
    pub fn resolve_blank(&self, options_height_cm: Option<f32>) -> Option<ResolvedBlank> {
        let height_cm = match options_height_cm {
            None => return None,
            Some(h) if h > 0.0 => h,
            Some(_) => self.answer_blank.height_cm,
        };
        Some(ResolvedBlank {
            style: self.answer_blank.style,
            height_mm: height_cm * 10.0,
        })
    }

    /// 按 id 取预设（前端下拉与 `/typeset/profiles` 共用）
    pub fn preset(id: &str) -> Option<LayoutSpec> {
        presets().into_iter().find(|p| p.id == id).map(|p| p.spec)
    }

    /// mode 默认 spec（§5.6）：讲义 A4 单栏、学生练习 A4 双栏、考卷 A3 对折双栏
    pub fn for_profile(profile: OutputProfile) -> LayoutSpec {
        let id = match profile {
            OutputProfile::Student => "a4_practice",
            OutputProfile::Teacher => "a4_lecture",
            OutputProfile::Exam => "a3_fold_exam",
        };
        Self::preset(id).unwrap_or_default()
    }
}

/// `GET /typeset/profiles` 的列表项
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[ts(export, export_to = "../frontend/src/api/types/layout.ts")]
pub struct ProfilePreset {
    pub id: String,
    /// 中文标签，前端下拉直接用
    pub label: String,
    pub spec: LayoutSpec,
}

/// 内置 4 预设（§6.1）
pub fn presets() -> Vec<ProfilePreset> {
    vec![
        ProfilePreset {
            id: "a4_lecture".into(),
            label: "A4 讲义 · 单栏".into(),
            spec: LayoutSpec {
                paper: Paper::A4,
                columns: 1,
                margins: Margins::default(),
                binding: None,
                header_footer: HeaderFooter::default(),
                profile: OutputProfile::Teacher,
                fonts: FontSpec::default(),
                answer_blank: BlankSpec::default(),
                color: ColorMode::Rich,
            },
        },
        ProfilePreset {
            id: "a4_practice".into(),
            label: "A4 练习 · 双栏".into(),
            spec: LayoutSpec {
                paper: Paper::A4,
                columns: 2,
                margins: Margins {
                    top_mm: 20.0,
                    right_mm: 15.0,
                    bottom_mm: 20.0,
                    left_mm: 15.0,
                    gutter_mm: 8.0,
                },
                binding: None,
                header_footer: HeaderFooter::default(),
                profile: OutputProfile::Student,
                fonts: FontSpec::default(),
                answer_blank: BlankSpec::default(),
                color: ColorMode::Rich,
            },
        },
        ProfilePreset {
            id: "a3_fold_exam".into(),
            label: "A3 对折 · 双栏考卷".into(),
            spec: LayoutSpec {
                paper: Paper::A3Fold,
                columns: 2,
                margins: Margins {
                    top_mm: 18.0,
                    right_mm: 16.0,
                    bottom_mm: 18.0,
                    left_mm: 16.0,
                    gutter_mm: 12.0,
                },
                binding: Some(Binding {
                    position: BindingPosition::CenterFold,
                    areas: BindingAreas {
                        school: true,
                        class_name: true,
                        name: true,
                        exam_no: true,
                    },
                }),
                header_footer: HeaderFooter {
                    header_title: false,
                    page_number: true,
                    odd_even_outer: true,
                },
                profile: OutputProfile::Exam,
                fonts: FontSpec::default(),
                answer_blank: BlankSpec::default(),
                color: ColorMode::Rich,
            },
        },
        ProfilePreset {
            id: "a3_tri_exam".into(),
            label: "A3 三栏考卷".into(),
            spec: LayoutSpec {
                paper: Paper::A3Tri,
                columns: 3,
                margins: Margins {
                    top_mm: 18.0,
                    right_mm: 14.0,
                    bottom_mm: 18.0,
                    left_mm: 14.0,
                    gutter_mm: 10.0,
                },
                binding: Some(Binding {
                    position: BindingPosition::Left,
                    areas: BindingAreas {
                        school: true,
                        class_name: true,
                        name: true,
                        exam_no: true,
                    },
                }),
                header_footer: HeaderFooter {
                    header_title: false,
                    page_number: true,
                    odd_even_outer: false,
                },
                profile: OutputProfile::Exam,
                fonts: FontSpec::default(),
                answer_blank: BlankSpec::default(),
                color: ColorMode::Rich,
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a4_single_column_width() {
        let spec = LayoutSpec::default();
        assert_eq!(spec.paper, Paper::A4);
        assert_eq!(spec.columns, 1);
        // 210 − 18 − 18 = 174
        assert!((spec.column_width_mm() - 174.0).abs() < 1e-6);
    }

    #[test]
    fn two_columns_pay_one_gutter() {
        let spec = LayoutSpec::preset("a4_practice").unwrap();
        // (210 − 15 − 15 − 8) / 2 = 86
        assert!((spec.column_width_mm() - 86.0).abs() < 1e-6);
        assert_eq!(spec.column_gutter_mm(), 8.0);
    }

    #[test]
    fn three_columns_pay_two_gutters() {
        let spec = LayoutSpec::preset("a3_tri_exam").unwrap();
        // (420 − 14 − 14 − 20) / 3 = 124
        assert!((spec.column_width_mm() - 124.0).abs() < 1e-6);
    }

    #[test]
    fn a3_papers_are_landscape() {
        assert_eq!(Paper::A4.size_mm(), (210, 297));
        assert_eq!(Paper::A3Fold.size_mm(), (420, 297));
        assert_eq!(Paper::A3Tri.size_mm(), (420, 297));
    }

    #[test]
    fn single_column_zeroes_the_gutter() {
        let spec = LayoutSpec {
            columns: 1,
            margins: Margins {
                gutter_mm: 8.0,
                ..Margins::default()
            },
            ..LayoutSpec::default()
        };
        assert_eq!(spec.column_gutter_mm(), 0.0);
        assert!((spec.column_width_mm() - 174.0).abs() < 1e-6);
    }

    #[test]
    fn default_spec_equals_a4_lecture_preset() {
        // 缺省与讲义预设只允许差 profile：版心几何漂了说明 Default 与预设各改了一半
        let lecture = LayoutSpec::preset("a4_lecture").unwrap();
        assert_eq!(lecture.profile, OutputProfile::Teacher);
        assert_eq!(LayoutSpec::default().profile, OutputProfile::Student);
        assert_eq!(
            LayoutSpec {
                profile: OutputProfile::Teacher,
                ..LayoutSpec::default()
            },
            lecture
        );
    }

    #[test]
    fn mode_defaults_map_to_distinct_layouts() {
        let teacher = LayoutSpec::for_profile(OutputProfile::Teacher);
        assert_eq!((teacher.paper, teacher.columns), (Paper::A4, 1));
        assert_eq!(teacher.binding, None);

        let student = LayoutSpec::for_profile(OutputProfile::Student);
        assert_eq!((student.paper, student.columns), (Paper::A4, 2));

        let exam = LayoutSpec::for_profile(OutputProfile::Exam);
        assert_eq!((exam.paper, exam.columns), (Paper::A3Fold, 2));
        assert_eq!(
            exam.binding.map(|b| b.position),
            Some(BindingPosition::CenterFold)
        );
    }

    #[test]
    fn options_decides_switch_and_height_spec_decides_style() {
        let spec = LayoutSpec::default(); // style=Lines, height=6cm
        // options 给了 3cm：留白，样式仍取 spec 的 lines，高度按 options
        let got = spec.resolve_blank(Some(3.0)).unwrap();
        assert_eq!(got.style, BlankStyle::Lines);
        assert!((got.height_mm - 30.0).abs() < 1e-6);
        // 开关在 options 手里：没开就不留白，spec 的兜底高度不许自己生效
        assert_eq!(spec.resolve_blank(None), None);
        // options 开了但高度没填（0 / 负数）：退回 spec 的 6cm
        assert!((spec.resolve_blank(Some(0.0)).unwrap().height_mm - 60.0).abs() < 1e-6);
        assert!((spec.resolve_blank(Some(-1.0)).unwrap().height_mm - 60.0).abs() < 1e-6);
        // 样式改点阵：只换 style，高度逻辑不变
        let dotted = LayoutSpec {
            answer_blank: BlankSpec {
                style: BlankStyle::Dots,
                ..BlankSpec::default()
            },
            ..LayoutSpec::default()
        };
        assert_eq!(
            dotted.resolve_blank(Some(2.0)).unwrap().style,
            BlankStyle::Dots
        );
    }

    #[test]
    fn class_field_serializes_as_class() {
        let json = serde_json::to_string(&BindingAreas {
            school: true,
            class_name: true,
            name: false,
            exam_no: false,
        })
        .unwrap();
        assert!(json.contains(r#""class":true"#), "{json}");
        assert!(!json.contains("class_name"), "{json}");
    }

    #[test]
    fn paper_and_style_use_snake_case_wire_names() {
        assert_eq!(serde_json::to_value(Paper::A3Fold).unwrap(), "a3_fold");
        assert_eq!(
            serde_json::to_value(ColorMode::PrintBlackOnly).unwrap(),
            "print_black_only"
        );
        assert_eq!(serde_json::to_value(BlankStyle::Dots).unwrap(), "dots");
    }

    #[test]
    fn unknown_and_missing_keys_never_fail_a_request() {
        // spec 覆盖是字段级的：前端可能带未来版本的键，未知键不该让整个请求 400
        let spec: LayoutSpec =
            serde_json::from_str(r#"{"paper":"a3_fold","unknown_future_key":1}"#).unwrap();
        assert_eq!(spec.paper, Paper::A3Fold);
        assert_eq!(spec.columns, 1, "缺字段应回到默认值");
        assert!(spec.header_footer.page_number);
        assert!(!spec.header_footer.header_title);

        // 嵌套对象同样是字段级覆盖：只给装订位，填涂区回默认
        let spec: LayoutSpec =
            serde_json::from_str(r#"{"binding":{"position":"center_fold"}}"#).unwrap();
        let binding = spec.binding.unwrap();
        assert_eq!(binding.position, BindingPosition::CenterFold);
        assert_eq!(binding.areas, BindingAreas::default());
    }

    #[test]
    fn preset_ids_and_serialized_shape_are_frozen() {
        let all = presets();
        assert_eq!(all.len(), 4);
        let ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            ["a4_lecture", "a4_practice", "a3_fold_exam", "a3_tri_exam"]
        );
        // 整份预设序列化后逐字符比：改字段名或默认值会在这里炸，而不是在前端下拉里静默漂移
        let got = serde_json::to_string_pretty(&all).unwrap();
        let want = include_str!("../../tests/snapshots/layout_presets.json");
        assert_eq!(
            got.trim(),
            want.trim(),
            "预设快照变了：确认是有意的再更新固件"
        );
    }

    /// 固件是编译期 `include_str!` 读进来的，改预设时会先编不过 —— 所以重生成走这条：
    /// `cargo test --lib typeset::spec -- --ignored`
    #[test]
    #[ignore = "只在有意更新快照时手动跑"]
    fn rewrite_layout_preset_snapshot() {
        let got = serde_json::to_string_pretty(&presets()).unwrap();
        std::fs::write("tests/snapshots/layout_presets.json", got + "\n").unwrap();
    }
}
