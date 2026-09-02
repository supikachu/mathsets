//! 版面参数模型（实施计划 §6.1）— 任务 T3.2
//!
//! `LayoutSpec` 是排版域（模块 B）的唯一入参：`/export/pdf` 按 mode 取内置预设，请求里带了
//! `spec` 就整体替换它（`profile` 仍由 mode 回填 —— 见 `export::pdf::resolve_spec`）。经 ts-rs
//! 导出 `frontend/src/api/types/layout.ts`（B6），前端 PDF 展开区的字段全部由它生成，不手写。
//!
//! 依赖方向：本文件一个 export 符号都不碰。`ExportMode → OutputProfile` 的翻译与
//! `ExamQuestion.answer_space` 的取值都在适配器 `export::pdf` 里做；本模块只交出合并规则
//! （B5：options 决定开关与高度，spec 决定样式）。

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// 纸张：A3 两档都按长边横置出纸；一张纸算几个「逻辑页」由 `Paper::logical_slots_per_sheet` 定
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

    /// 一张物理纸承载几**逻辑页**（R4）
    ///
    /// 只有 A3 对折例外：左右半张各自是一页 A4，读者按「左半 → 右半」读，页码与奇偶外侧都得
    /// 按半张计。A4 与 A3 三栏都是「一张纸一页」—— 那里的栏只是栏，不多造页码。
    pub fn logical_slots_per_sheet(self) -> u8 {
        match self {
            Self::A3Fold => 2,
            Self::A4 | Self::A3Tri => 1,
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

/// 密封装订带的宽度（mm，T4.8）
///
/// 带子里要并排放下竖虚线、旋转 90° 的「密封装订线，请勿折叠」与学校 / 班级 / 姓名 / 考号
/// 四个填涂字段，20mm 是这些内容转成竖排后能塞下的下限。[`LayoutSpec::margin_left_mm`] 在
/// `Left` 时把它加到左边距之外，[`LayoutSpec::column_gutter_mm`] 在 `CenterFold` 时把它作为
/// 栏距下限 —— 装订带要占中线，中线版心就得给它让路。
pub const SEALING_BAND_MM: f32 = 20.0;

/// 页眉页脚开关。页码按**逻辑页**计（T4.7 / R4：A3 对折的一张纸报两页号），奇偶外侧对齐同期
/// 落地；只剩动态页眉取当前大题名（T4.10）还没接
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
    /// 数学字体：两个出口当前都没有落点 —— typst 0.15 的方程元素把自己的字体硬设成
    /// New Computer Modern Math（见 `typeset::typst_gen` 的实测说明），docx 的 OMML 由
    /// Word 自行选字体。字段按 §6.1 定齐，等真能落地时再生效
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

/// 留白样式与默认高度。是否留白与最终样式按「题级 → 卷级 options → 本字段」取用（B5）
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
    /// 单栏可用版心宽（mm）= 纸宽 − 生效左边距 − 右边距 − (n−1) × 生效栏距，再除以 n
    ///
    /// 两个「生效」都带装订（[`margin_left_mm`](Self::margin_left_mm) /
    /// [`column_gutter_mm`](Self::column_gutter_mm)）：装订带吃掉的每一份宽度必须同时从栏宽里
    /// 扣掉，否则选项栅格与配图裁宽用的就不是纸上那一栏的真实宽度 —— 那是比排不满更难看的错。
    ///
    /// docx 侧另有一套 twips 口径（T2.7），两边都只有一处实现，漂了用例会先炸
    pub fn column_width_mm(&self) -> f32 {
        let (w, _) = self.paper.size_mm();
        let n = self.columns.max(1) as f32;
        let gutters = (n - 1.0) * self.column_gutter_mm();
        ((w as f32) - self.margin_left_mm() - self.margins.right_mm - gutters) / n
    }

    /// 各栏左沿的绝对 x（mm，从**纸**的左边缘量）
    ///
    /// 与 [`column_width_mm`](Self::column_width_mm) 同一条算式，只是把逐栏累加写出来。取这个
    /// 原点是有意的：typst 的 `Location.position().x` 与帧树的 `x_mm` 都从纸边量起（实测标记落在
    /// x = 生效左边距、栏内正文帧的 x 与栏左沿同值），页眉要靠它把绝对 x 反推成栏号（T4.10），
    /// 差一个左边距就把每一栏都算到左边那一栏去。
    pub fn column_lefts_mm(&self) -> Vec<f32> {
        let step = self.column_width_mm() + self.column_gutter_mm();
        (0..self.columns.max(1))
            .map(|i| self.margin_left_mm() + step * i as f32)
            .collect()
    }

    /// 生效的左边距（mm）：`Left` 装订带不是装饰，它真的吃掉版面前这条带子，正文整块右移
    pub fn margin_left_mm(&self) -> f32 {
        match self.binding {
            Some(b) if b.position == BindingPosition::Left => {
                self.margins.left_mm + SEALING_BAND_MM
            }
            _ => self.margins.left_mm,
        }
    }

    /// 生效的栏间距（mm）：单栏恒为 0，避免母版里写出没有意义的 `column-gutter`；
    /// `CenterFold` 时不得低于装订带 —— 对折的密封线走版心中线，栏距不给够它就会压上正文
    pub fn column_gutter_mm(&self) -> f32 {
        if self.columns <= 1 {
            return 0.0;
        }
        match self.binding {
            Some(b) if b.position == BindingPosition::CenterFold => {
                self.margins.gutter_mm.max(SEALING_BAND_MM)
            }
            _ => self.margins.gutter_mm,
        }
    }

    /// 一张物理纸上的逻辑页格数（R4）：页脚要切成几格、页码要不要 ×2 全看这里
    ///
    /// 取纸张口径与实际栏数的较小值：A3 对折排成单栏时「半张 = 一页」根本不成立，
    /// 与其编出一个对不上折痕的页号，不如老实只编一个。
    pub fn logical_slots(&self) -> u8 {
        self.paper
            .logical_slots_per_sheet()
            .min(self.columns.max(1))
    }

    /// 版面侧的留白折算：`None` = 高度没给 → `None`；`Some(非正数)` 视为「没填」退回本 spec 的
    /// 兜底高度；样式恒取 spec。
    ///
    /// 这里只是**单位折算 + 兜底**，最终要不要留白、用哪套样式归
    /// [`crate::typeset::blocks::blank`]：那里才有「题级 > 卷级 options > 版面」的三级优先级，
    /// options 侧的样式按 B5 盖过本函数的返回。
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
        // 三栏卷是左侧装订：(420 − (14+20) − 14 − 2×10) / 3 = 117.33
        assert!((spec.column_width_mm() - 352.0 / 3.0).abs() < 1e-4);
    }

    #[test]
    fn center_fold_gutter_floors_at_the_sealing_band() {
        let fold = LayoutSpec::preset("a3_fold_exam").unwrap();
        // 预设写的 12mm 栏距放不下 20mm 装订带：对折的密封线走版心中线，栏距得兜高
        assert_eq!(fold.margins.gutter_mm, 12.0);
        assert_eq!(fold.column_gutter_mm(), SEALING_BAND_MM);
        // (420 − 16 − 16 − 20) / 2 = 184
        assert!((fold.column_width_mm() - 184.0).abs() < 1e-6);
        // 折痕 = 左边距 + 栏宽 + 半栏距 = 210 = 纸宽中线，页码格与装订带都按它对齐
        let fold_x = fold.margin_left_mm() + fold.column_width_mm() + fold.column_gutter_mm() / 2.0;
        assert!((fold_x - 210.0).abs() < 1e-6);
        // 对折的带子吃栏距，不吃页边距
        assert_eq!(fold.margin_left_mm(), fold.margins.left_mm);
    }

    #[test]
    fn left_binding_shifts_the_text_right_by_the_band() {
        let mut spec = LayoutSpec::preset("a4_lecture").unwrap();
        let plain = spec.column_width_mm();
        spec.binding = Some(Binding {
            position: BindingPosition::Left,
            areas: BindingAreas::default(),
        });
        assert_eq!(
            spec.margin_left_mm(),
            spec.margins.left_mm + SEALING_BAND_MM
        );
        // 正文整块右移：栏宽少掉的正是那条带子
        assert!((plain - spec.column_width_mm() - SEALING_BAND_MM).abs() < 1e-6);
        // 左侧装订不碰中线，栏距口径不变（单栏更是恒为 0）
        assert_eq!(spec.column_gutter_mm(), 0.0);
    }

    #[test]
    fn only_the_fold_multiplies_page_numbers() {
        assert_eq!(Paper::A3Fold.logical_slots_per_sheet(), 2);
        assert_eq!(Paper::A4.logical_slots_per_sheet(), 1);
        assert_eq!(Paper::A3Tri.logical_slots_per_sheet(), 1);
        // A3 三栏的三栏只是栏：一张纸一个页号
        assert_eq!(
            LayoutSpec::preset("a3_tri_exam").unwrap().logical_slots(),
            1
        );
        assert_eq!(
            LayoutSpec::preset("a3_fold_exam").unwrap().logical_slots(),
            2
        );
        // 对折排成单栏时「半张 = 一页」不成立，与其编出错号不如老实编一个
        let single = LayoutSpec {
            columns: 1,
            ..LayoutSpec::preset("a3_fold_exam").unwrap()
        };
        assert_eq!(single.logical_slots(), 1);
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
