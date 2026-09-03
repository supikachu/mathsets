//! typst 编译器：虚拟文件系统 + PDF / SVG 出口（任务分解 T3.5）
//!
//! 依赖方向与本模块其余文件一致：只吃 typst 与标准库，一个 export 符号都不碰。
//! 源码由 `typst_gen`（T3.6）生成，本模块只负责把它连同素材交给 typst 并收集诊断。
//!
//! ## 字体（R6）
//! **运行时读目录**，禁 `include_bytes!`：`typst_assets::fonts()`（要 `fonts` feature ——
//! 它不在 typst-assets 的 default 里，漏开就拿到空迭代器，一个字体都没有）加上调用方
//! 传进来的 `font_dirs`（思源宋体/黑体 SC）。整个进程按「目录集」记忆化一次：83MB 的
//! OTF 逐请求解析，单卷 500ms 的目标会直接破产。缺中文字体不报错，回退 + 记 warning
//! （§13.4）—— 豆腐块是可诊断的症状，500 是致命的。
//!
//! ## 素材（B1）
//! `/uploads/**` 映射到 `upload_dir`；外链图片由调用方抓成字节后按 `/ext/<n>.<ext>`
//! 注入。这里按**序号**命名而不是 URL 哈希：typst 的 `FileId` 是全局 interner，上限
//! 65535 且永不回收，哈希名会让它随请求单调增长。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use ecow::{EcoVec, eco_format};
use typst::Library as TypstLibrary;
use typst::diag::{FileError, FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::layout::{Frame, FrameItem, Point};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook, FontInfo};
use typst::utils::LazyHash;
use typst::visualize::Geometry;
use typst::{Library, LibraryExt, World, compile};
use typst_layout::PagedDocument;

/// 一次编译的输入。
pub struct CompileRequest<'a> {
    /// `typst_gen` 产出的完整源码（含母版与 mitex 定义块）
    pub source: &'a str,
    /// `/uploads/**` 的落盘根目录（`config.upload_dir`）
    pub upload_dir: &'a Path,
    /// 字体搜索目录（R6）
    pub font_dirs: &'a [PathBuf],
    /// 外链图片字节（B1）：`(虚拟路径, 字节)`，路径须为 `/ext/<n>.<ext>`。
    /// 路径收 `String` 而不是 `&str`：调用方分配序号时天然持有 String，收 `&str` 就逼它
    /// 为了换个类型把十几 MB 的字节整份 clone 一遍。
    pub injected: &'a [(String, Vec<u8>)],
}

/// 编译产物 + typst 自己提出的告警（缺字体、弃用写法等）。
pub struct Compiled<T> {
    pub output: T,
    pub warnings: Vec<String>,
}

/// 手写 Debug：`T` 是 PDF 字节或整页 SVG，derive 出来的话一行日志就是几十万字节的原文。
impl<T> std::fmt::Debug for Compiled<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Compiled")
            .field("output", &"<hidden>")
            .field("warnings", &self.warnings)
            .finish()
    }
}

/// 编译失败：typst 的诊断原文，按出现顺序展平。
#[derive(Debug, Clone)]
pub struct CompileError {
    pub diagnostics: Vec<String>,
}

impl CompileError {
    /// 给 Issue / 日志用的一行摘要（首条诊断 + 总条数）
    pub fn summary(&self) -> String {
        let Some(first) = self.diagnostics.first() else {
            return "编译失败（无诊断）".to_string();
        };
        if self.diagnostics.len() == 1 {
            format!("编译失败：{first}")
        } else {
            format!("编译失败：{first}（共 {} 条诊断）", self.diagnostics.len())
        }
    }
}

/// 源码 → PDF 字节，一步到位。
///
/// 生产路径不走它：`export::pdf` 把 [`compile_paged`] 与取样拆开，好让一份卷子只编一次。
/// 留着是给测试与探针用的便捷入口。
pub fn compile_pdf(req: &CompileRequest) -> Result<Compiled<Vec<u8>>, CompileError> {
    let compiled = compile_paged(req)?;
    let bytes = pdf_bytes(&compiled.output)?;
    Ok(Compiled {
        output: bytes,
        warnings: compiled.warnings,
    })
}

/// 已有的 `PagedDocument` → PDF 字节。
///
/// 拆出来是为了「编译一次、多处取样」：预览（T5.2）同一次编译既要帧树（预检）又要产物，
/// 编两遍既慢，又可能因为字体池变化给出两份不一致的卷。
pub fn pdf_bytes(doc: &PagedDocument) -> Result<Vec<u8>, CompileError> {
    typst_pdf::pdf(doc, &typst_pdf::PdfOptions::default()).map_err(to_error)
}

/// 源码 → 逐页 SVG，一步到位（消费者同 [`compile_pdf`]：测试与探针）。
pub fn compile_svg_pages(req: &CompileRequest) -> Result<Compiled<Vec<String>>, CompileError> {
    let compiled = compile_paged(req)?;
    Ok(Compiled {
        output: svg_pages(&compiled.output),
        warnings: compiled.warnings,
    })
}

/// 已有的 `PagedDocument` → 逐页 SVG 源码。
///
/// **自包含**：typst-svg 0.15.1 把每个字形描成 `<path>`（字体里带 SVG 表的走 SVG，位图字形走
/// 内嵌位图），整页不写 `@font-face` —— `SvgOptions` 只有 `render_bleed` 与 `pretty` 两个开关。
/// 对预览是好事：浏览器没装思源也能看到将要印出来的那套字；代价是页面上的字**不可选中、不可
/// 检索**，前端（T5.5）把每页当图片摆出来即可，别指望在预览里复制题干。
pub fn svg_pages(doc: &PagedDocument) -> Vec<String> {
    let options = typst_svg::SvgOptions::default();
    doc.pages()
        .iter()
        .map(|page| typst_svg::svg(page, &options))
        .collect()
}

/// 编译到 `PagedDocument`：World 之外的失败统一成 typst 诊断。
///
/// 公开是为了 [`rendered_runs`] 能拿到帧树 —— 版面文字只能从 `Page::frame` 里读。
pub fn compile_paged(req: &CompileRequest) -> Result<Compiled<PagedDocument>, CompileError> {
    let world = TypesetWorld::new(req);
    let warned = compile::<PagedDocument>(&world);
    Ok(Compiled {
        output: warned.output.map_err(to_error)?,
        warnings: flatten_warnings(&warned.warnings),
    })
}

// ---------------------------------------------------------------- 帧树回读

/// 版面上真正画出来的一段字：文本 + 画它的那个字体族名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedRun {
    pub text: String,
    pub family: String,
}

/// 一段字连同它在**页内**的落点与宽度（毫米，原点 = 页面左上角）。
///
/// `w_mm` 取 typst `TextItem::width()` = 各字形 `x_advance` 之和：够宽到把「这段字印到哪结束」
/// 判准（印前预检的溢流判据要的是包围盒，光有锚点等于什么都没查），但它**不是墨迹范围**——
/// 尾部空格照样计入，字形向右越界伸出（斜体、连字）不计入。
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedRun {
    pub x_mm: f64,
    pub y_mm: f64,
    pub w_mm: f64,
    pub run: RenderedRun,
}

/// 一枚画在版面上的图片：等比缩放后的实际尺寸 + 页内落点（毫米，原点 = 页面左上角）。
///
/// 只有**栅格**图会在这里现身 —— SVG 在 typst 里被转成矢量 `Group`，帧树中没有 `Image` 项
/// （实测）。要断言图片几何，测试得喂 PNG。
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedImage {
    pub x_mm: f64,
    pub y_mm: f64,
    pub w_mm: f64,
    pub h_mm: f64,
}

/// 一笔画在版面上的**直线**（typst 侧 `Geometry::Line` 那一种 Shape）：起点 + 相对位移 +
/// 线宽 + 已折算成绝对长度的 dash 数组（`None` = 实线），全部毫米、原点 = 页面左上角。
///
/// 留白三样式（T4.4 的「编译视觉正确」）只能这样验：留白块是 `height: h, clip: true`，
/// 源码里写了 n 条线不代表纸上画了 n 条 —— 溢出的一两条会被静静裁掉。
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedLine {
    pub x_mm: f64,
    pub y_mm: f64,
    pub dx_mm: f64,
    pub dy_mm: f64,
    pub thickness_mm: f64,
    pub dash_mm: Option<Vec<f64>>,
}

/// 一次帧树遍历的三类落点：[`placed_pages`]、[`placed_images`]、[`placed_lines`] 各取所需
#[derive(Debug, Default)]
struct Placed {
    runs: Vec<PlacedRun>,
    images: Vec<PlacedImage>,
    lines: Vec<PlacedLine>,
}

/// 逐页收集版面文字（下标 = 物理页，从 0 起）。
///
/// 防跨页（T4.5）的断言口径是「这两段字在同一页」，扁平列表表达不了，所以页归属留在这里。
/// 粒度是**物理页**：A3 双栏时两栏同属一个条目，这对「不许腰斩」正好，对 T4.7 的
/// 「逻辑页 / 左右栏」还得更细的坐标判定（用 [`placed_pages`]）。
/// 一段 `TextItem` 不会横跨两页 —— typst 只在行与行之间断页，帧树里的文字段总是完整落在
/// 某一页内，因此「按页分组」不需要任何近似。
pub fn rendered_pages(doc: &PagedDocument) -> Vec<Vec<RenderedRun>> {
    placed_pages(doc)
        .into_iter()
        .map(|page| page.into_iter().map(|placed| placed.run).collect())
        .collect()
}

/// 逐页收集版面文字**及其落点**（下标 = 物理页，从 0 起）。
///
/// 为什么要走帧树而不是搜 SVG/PDF 字节：typst 的 SVG 与 PDF 都把汉字画成**矢量轮廓**，
/// 文件里根本没有「中文字」这三个字的明文，搜关键词恒为 false（实测踩过）。帧树里的
/// `TextItem.text` 才是明文，而且同时带 `font` —— 于是「这段字到底出没出现」和
/// 「这个字是哪个字体画的（豆腐块判定）」两件事共用一次遍历。
///
/// 坐标口径：`Frame::items()` 本来就是 `(Point, FrameItem)`，段的位置 = 自身 Point +
/// 各层 `GroupItem::transform` 的平移量累加。只累加平移、不乘缩放与旋转 —— 栅格与缩进给出的
/// 平移正是「第几列 / 第几行」的判据（T4.2 的选项栅格、T4.7 的答案分区都靠它）。
/// 代价是**旋转过的内容**（T4.8 装订带上那行 `rotate(90deg)`）在这里报的是摆放锚点、不是它印在
/// 纸上的那条包围盒（实测 x 恰等于 `place` 的 dx）。所以旋转内容只能断「出没出现、锚在哪」，
/// 想要长度得另想办法。
///
/// 只走 `Group` 递归；文字与栅格图共用这一次遍历，`Link` / `Tag` 与三者无关。
pub fn placed_pages(doc: &PagedDocument) -> Vec<Vec<PlacedRun>> {
    place_pages(doc).into_iter().map(|p| p.runs).collect()
}

/// 逐页收集版面上的图片**及其实际画出的尺寸与落点**（下标 = 物理页，从 0 起）。
///
/// 「图列不失宽」只能这样验：图在纸上就是一个 `Size`，源码里的 `width:` 说了不算。
pub fn placed_images(doc: &PagedDocument) -> Vec<Vec<PlacedImage>> {
    place_pages(doc).into_iter().map(|p| p.images).collect()
}

/// 逐页收集版面上画出的**直线**（下标 = 物理页，从 0 起）。
///
/// 留白三样式（T4.4）的行数、行距、点距全靠它：横线是实线 Shape、点阵是带 dash 的 Shape、
/// 纯空白一个都不该有，而这三件事在源码字符串里根本区分不出来。
pub fn placed_lines(doc: &PagedDocument) -> Vec<Vec<PlacedLine>> {
    place_pages(doc).into_iter().map(|p| p.lines).collect()
}

/// 逐页的纸张尺寸（毫米，下标 = 物理页）。
///
/// 印前预检（T5.1 / R12）的溢流判据要的是**纸边**，而 `Page::frame` 的尺寸就是纸边：实测
/// `#set page(width: 300mm, height: 120mm)` 报 300.00×120.00。不用 `spec.paper` 反推 ——
/// 母版分离（T4.9）之后首页与正文页本就可能不同尺寸，逐页现读才是事实。
pub fn page_sizes(doc: &PagedDocument) -> Vec<(f64, f64)> {
    doc.pages()
        .iter()
        .map(|page| {
            let s = page.frame.size();
            (s.x.to_mm(), s.y.to_mm())
        })
        .collect()
}

fn place_pages(doc: &PagedDocument) -> Vec<Placed> {
    doc.pages()
        .iter()
        .map(|page| {
            let mut out = Placed::default();
            walk_frame(&page.frame, Point::zero(), &mut out);
            out
        })
        .collect()
}

fn walk_frame(frame: &Frame, at: Point, out: &mut Placed) {
    for (pos, item) in frame.items() {
        let here = *pos + at;
        match item {
            FrameItem::Text(text) => out.runs.push(PlacedRun {
                x_mm: here.x.to_mm(),
                y_mm: here.y.to_mm(),
                w_mm: text.width().to_mm(),
                run: RenderedRun {
                    text: text.text.to_string(),
                    family: text.font.info().family.clone(),
                },
            }),
            FrameItem::Image(_, size, _) => out.images.push(PlacedImage {
                x_mm: here.x.to_mm(),
                y_mm: here.y.to_mm(),
                w_mm: size.x.to_mm(),
                h_mm: size.y.to_mm(),
            }),
            FrameItem::Group(group) => {
                let t = &group.transform;
                walk_frame(&group.frame, here + Point::new(t.tx, t.ty), out);
            }
            FrameItem::Shape(shape, _) => {
                // 只认直线：矩形 / 曲线（圆、Bézier）在留白判据里用不上，收进来只会添噪
                if let Geometry::Line(to) = shape.geometry {
                    let stroke = shape.stroke.as_ref();
                    out.lines.push(PlacedLine {
                        x_mm: here.x.to_mm(),
                        y_mm: here.y.to_mm(),
                        dx_mm: to.x.to_mm(),
                        dy_mm: to.y.to_mm(),
                        thickness_mm: stroke.map_or(0.0, |s| s.thickness.to_mm()),
                        dash_mm: stroke
                            .and_then(|s| s.dash.as_ref())
                            .map(|d| d.array.iter().map(|l| l.to_mm()).collect()),
                    });
                }
            }
            _ => {}
        }
    }
}

/// 深度优先走帧树，收集所有落到版面的文字段。
pub fn rendered_runs(doc: &PagedDocument) -> Vec<RenderedRun> {
    rendered_pages(doc).into_iter().flatten().collect()
}

/// 模板里要用的中文字体族名（`CJK_FAMILIES[0]` 正文、`[1]` 标题），缺一即回退 + 告警
pub const CJK_FAMILIES: &[&str] = &["Source Han Serif SC", "Source Han Sans SC"];

/// 中文字体齐不齐，返回缺掉的族名。
///
/// 只查 CJK：拉丁与数学字体由 typst-assets 兜住，缺了表现为字形回退，不是豆腐块。
/// `FontBook` 的族名索引是小写键，所以查询必须转小写 —— 传 `"Source Han Serif SC"`
/// 原样去查会恒为「缺」（实测）。
pub fn missing_cjk_families(book: &FontBook) -> Vec<&'static str> {
    CJK_FAMILIES
        .iter()
        .copied()
        .filter(|f| !book.contains_family(&f.to_lowercase()))
        .collect()
}

/// 这组目录解析出来的字体池里缺哪些中文字体（§13.4：回退不报错，但必须可告警）。
///
/// 走 [`font_pool`] 而不是新解析一遍：目录集相同就直接命中缓存。调用方是 `/export/pdf`
/// 的 handler —— 豆腐块只有教师能看见，得让它进 `X-Export-Warnings`。
pub fn missing_cjk_fonts(dirs: &[PathBuf]) -> Vec<&'static str> {
    missing_cjk_families(&font_pool(dirs).book)
}

// ---------------------------------------------------------------- 字体池

/// 字体搜索目录（R6）：默认仓库里的 `assets/fonts`，`TYPESET_FONT_DIRS` 按平台路径分隔符
/// 覆盖它（部署时字体放在二进制外面）。
///
/// 放在这儿而不是 `AppConfig`：全仓有十几处 `AppState::new`，为一个「只有排版出口用」的
/// 路径给它们统统加字段不划算。目录集的解析结果由 [`font_pool`] 记忆化，重复调用零成本。
pub fn font_dirs() -> Vec<PathBuf> {
    match std::env::var_os("TYPESET_FONT_DIRS") {
        Some(v) if !v.is_empty() => std::env::split_paths(&v)
            .filter(|p| !p.as_os_str().is_empty())
            .collect(),
        _ => vec![PathBuf::from("assets/fonts")],
    }
}

/// 解析过一次的字体集：`faces[i]` 与 `book` 里第 i 条 `FontInfo` 一一对应。
struct FontPool {
    book: LazyHash<FontBook>,
    /// `(数据, 该数据内的 face 序号)` —— `FontInfo` 自己不记 face 序号，
    /// 只能由我们按加入顺序平行保存。
    faces: Vec<(Bytes, u32)>,
}

static POOL_CACHE: OnceLock<Mutex<HashMap<Vec<PathBuf>, Arc<FontPool>>>> = OnceLock::new();

/// 同一组目录只解析一次。锁中毒时照用：缓存里没有不变式，最坏是多解析一遍。
fn font_pool(dirs: &[PathBuf]) -> Arc<FontPool> {
    let cache = POOL_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(pool) = guard.get(dirs) {
        return pool.clone();
    }
    let pool = Arc::new(build_pool(dirs));
    guard.insert(dirs.to_vec(), pool.clone());
    pool
}

fn build_pool(dirs: &[PathBuf]) -> FontPool {
    let mut infos = Vec::new();
    let mut faces: Vec<(Bytes, u32)> = Vec::new();
    let mut seen: HashSet<(u64, u32)> = HashSet::new();

    let mut push = |data: &[u8]| {
        // face 序号取自 `FontInfo::iter` 的枚举序：它按 0..fonts_in_collection 逐个
        // 产出，仅在某个 face 解析失败时才会与真实序号错位（集合字体里极罕见）。
        for (index, info) in FontInfo::iter(data).enumerate() {
            let face = index as u32;
            if !seen.insert((finger(data), face)) {
                continue;
            }
            infos.push(info);
            faces.push((Bytes::new(data.to_vec()), face));
        }
    };

    for data in typst_assets::fonts() {
        push(data);
    }
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("otf" | "ttf" | "ttc" | "woff2")
            ) {
                continue;
            }
            let Ok(data) = std::fs::read(&path) else {
                continue;
            };
            push(&data);
        }
    }

    FontPool {
        book: LazyHash::new(FontBook::from_infos(infos)),
        faces,
    }
}

/// 同一份字体可能在两个目录各放一份：用长度 + 首尾字节粗去重
fn finger(data: &[u8]) -> u64 {
    let mut h = data.len() as u64;
    for byte in data.iter().take(64).chain(data.iter().rev().take(64)) {
        h = h.wrapping_mul(1_000_003) ^ u64::from(*byte);
    }
    h
}

// ---------------------------------------------------------------- World

/// typst 的 `World`：一份虚拟源码 + 素材 + 进程级字体池。
struct TypesetWorld {
    library: LazyHash<TypstLibrary>,
    pool: Arc<FontPool>,
    main: FileId,
    sources: HashMap<FileId, Source>,
    files: HashMap<FileId, Bytes>,
    upload_root: PathBuf,
}

impl TypesetWorld {
    fn new(req: &CompileRequest) -> Self {
        let pool = font_pool(req.font_dirs);
        let main = virtual_id("/main.typ");
        let mut sources = HashMap::new();
        sources.insert(main, Source::new(main, req.source.to_string()));

        let mut files = HashMap::new();
        for (path, data) in req.injected {
            files.insert(virtual_id(path), Bytes::new(data.clone()));
        }

        Self {
            // 字体集只经 World::book / World::font 提供：typst 0.15 的 Library 不带 book
            library: LazyHash::new(Library::default()),
            pool,
            main,
            sources,
            files,
            upload_root: req.upload_dir.to_path_buf(),
        }
    }
}

impl World for TypesetWorld {
    fn library(&self) -> &LazyHash<TypstLibrary> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.pool.book
    }

    fn main(&self) -> FileId {
        self.main
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.sources
            .get(&id)
            .cloned()
            .ok_or_else(|| FileError::NotFound(id.vpath().get_without_slash().into()))
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if let Some(bytes) = self.files.get(&id) {
            return Ok(bytes.clone());
        }
        let vpath = id.vpath();
        let rel = vpath.get_without_slash();
        let Some(sub) = rel.strip_prefix("uploads/") else {
            // 只服务 uploads/：注入型素材（`/ext/<n>`）上面已经查过了
            return Err(FileError::NotFound(rel.into()));
        };
        // realize 逐段校验并拒绝任何能逃出 root 的写法（Windows 的 `\`、盘符也算）——
        // 文件名来自库里存的 URL，不能当可信路径直接 join。
        let path = VirtualPath::new(sub)
            .map_err(|e| FileError::Other(Some(eco_format!("{e}"))))?
            .realize(&self.upload_root)
            .map_err(FileError::Realize)?;
        std::fs::read(&path)
            .map(Bytes::new)
            .map_err(|e| FileError::from_io(e, &path))
    }

    fn font(&self, index: usize) -> Option<Font> {
        let (data, face) = self.pool.faces.get(index)?;
        Font::new(data.clone(), *face)
    }

    /// 一律 `None`：生成的源码不写 `datetime`，考卷上的日期由 Rust 侧格式化后注入。
    /// 返回 None 只会在源码调用 `datetime.today()` 时让它报错，不影响排版本身。
    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        None
    }
}

/// 把 `/main.typ` 这样的虚拟路径 intern 成 `FileId`
fn virtual_id(path: &str) -> FileId {
    let vpath =
        VirtualPath::new(path).unwrap_or_else(|_| VirtualPath::new("/invalid.typ").expect("常量"));
    FileId::new(RootedPath::new(VirtualRoot::Project, vpath))
}

fn to_error(errors: EcoVec<SourceDiagnostic>) -> CompileError {
    CompileError {
        diagnostics: errors.iter().map(flatten_one).collect(),
    }
}

fn flatten_warnings(warnings: &EcoVec<SourceDiagnostic>) -> Vec<String> {
    let mut out: Vec<String> = warnings.iter().map(flatten_one).collect();
    out.sort();
    out.dedup();
    out
}

/// 诊断展平：消息 + 首个提示。不带行列 —— 源码是我们生成的，行号对教师无意义。
fn flatten_one(diag: &SourceDiagnostic) -> String {
    match diag.hints.first() {
        Some(hint) => format!("{}（{}）", diag.message, hint.v),
        None => diag.message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1×1 灰块 SVG：走 typst 的 svg 解码分支，省掉一张二进制固件。
    /// 用 `r##` 而不是 `r#`：内容里的 `fill="#333"` 含 `"#`，会提前终结单层原始字符串。
    const DOT_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="#333"/></svg>"##;

    fn request<'a>(source: &'a str, dirs: &'a [PathBuf]) -> CompileRequest<'a> {
        CompileRequest {
            source,
            upload_dir: Path::new("uploads"),
            font_dirs: dirs,
            injected: &[],
        }
    }

    #[test]
    fn hello_world_compiles_to_nonempty_pdf() {
        let dirs = font_dirs();
        let out = compile_pdf(&request("hello world", &dirs)).unwrap();
        assert!(
            out.output.starts_with(b"%PDF"),
            "产物不是 PDF：{} 字节",
            out.output.len()
        );
        assert!(out.output.len() > 800, "PDF 小得可疑：{}", out.output.len());
        assert!(out.warnings.is_empty(), "{:?}", out.warnings);
    }

    #[test]
    fn chinese_math_and_image_source_compiles() {
        let source = format!(
            concat!(
                "{}\n",
                "#set text(font: \"{}\", size: 10.5pt)\n",
                "已知集合 $A = \\{{1,2\\}}$，则（　）\n",
                "#image(\"/ext/0.svg\", width: 4pt)\n",
                "矩阵 $ mitexarray(arg0: c c ,1 zws ,2 ) $ 与 #textmath[甲]",
            ),
            crate::typeset::math::MITEX_PREAMBLE,
            CJK_FAMILIES[1],
        );
        let injected = [("/ext/0.svg".to_string(), DOT_SVG.as_bytes().to_vec())];
        let dirs = font_dirs();
        let req = CompileRequest {
            source: &source,
            upload_dir: Path::new("uploads"),
            font_dirs: &dirs,
            injected: &injected,
        };
        let out = compile_pdf(&req).expect("中文 + 公式 + 图片应当编译成功");
        assert!(out.output.starts_with(b"%PDF"));
        let font_warnings: Vec<&String> = out
            .warnings
            .iter()
            .filter(|w| w.to_lowercase().contains("font"))
            .collect();
        assert!(
            font_warnings.is_empty(),
            "不该有字体告警：{:?}",
            font_warnings
        );
    }

    /// T3.4 与 T3.5 的合流证据：静态守卫（`unresolved_name`）判能过的公式，必须真的
    /// 编译得动。语料与 `math::dump_corpus` 共用一份。逐枚单独编译要几十次 typst 启动，
    /// 所以先整批编一次，失败时再逐条定位。
    #[test]
    fn converted_corpus_compiles() {
        use crate::typeset::math::{CORPUS, MITEX_PREAMBLE, to_typst};

        let dirs = font_dirs();
        let convert = |latex: &str| {
            to_typst(latex, true).unwrap_or_else(|reason| panic!("{latex} 转换失败：{reason}"))
        };
        let source = format!(
            "{MITEX_PREAMBLE}\n{}\n",
            CORPUS
                .iter()
                .map(|l| convert(l))
                .collect::<Vec<_>>()
                .join("\n")
        );

        if let Err(err) = compile_pdf(&request(&source, &dirs)) {
            let culprits: Vec<String> = CORPUS
                .iter()
                .filter_map(|latex| {
                    let one = format!("{MITEX_PREAMBLE}\n{}\n", convert(latex));
                    compile_pdf(&request(&one, &dirs))
                        .err()
                        .map(|e| format!("{latex} -> {}", e.summary()))
                })
                .collect();
            panic!("整批编译失败：{}；逐条定位为 {culprits:?}", err.summary());
        }
    }

    #[test]
    fn diagnostics_are_enumerable() {
        let dirs = font_dirs();
        let err = compile_pdf(&request("#no-such-function(1)", &dirs)).unwrap_err();
        assert!(!err.diagnostics.is_empty());
        assert!(
            err.diagnostics[0].contains("no-such-function"),
            "诊断没带上可疑标识符：{:?}",
            err.diagnostics
        );
        assert!(err.summary().starts_with("编译失败："));
    }

    #[test]
    fn summary_counts_extra_diagnostics() {
        // typst 遇错即中止求值，一次编译通常只有一条诊断；多条是我们自己追加的情况，
        // 措辞得把总数带上。
        let one = CompileError {
            diagnostics: vec!["缺少字体".to_string()],
        };
        assert_eq!(one.summary(), "编译失败：缺少字体");
        let many = CompileError {
            diagnostics: vec!["a".to_string(), "b".to_string(), "c".to_string()],
        };
        assert_eq!(many.summary(), "编译失败：a（共 3 条诊断）");
        assert_eq!(
            CompileError {
                diagnostics: vec![]
            }
            .summary(),
            "编译失败（无诊断）"
        );
    }

    /// 字体池摸底：打印池里出现过的族名，用来核对 `CJK_FAMILIES` 抄对没有。
    /// `cargo test --lib typeset::compiler::tests::dump_font_families -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn dump_font_families() {
        let pool = build_pool(&font_dirs());
        for (family, ids) in pool.book.families() {
            println!("{family}: {} face", ids.count());
        }
        println!(
            "face 数={} 缺中文字体={:?}",
            pool.faces.len(),
            missing_cjk_families(&pool.book)
        );
    }

    #[test]
    fn cjk_fonts_actually_load_from_assets_dir() {
        let pool = build_pool(&font_dirs());
        assert!(
            pool.faces.len() > typst_assets::fonts().count(),
            "assets/fonts 一个字体都没加进来（LFS 未拉取？）：face 数 {}",
            pool.faces.len()
        );
        assert!(
            missing_cjk_families(&pool.book).is_empty(),
            "中文字体没注册上"
        );
        // World::font 的下标口径：book 里第 i 条 ↔ faces[i]
        assert!(pool.book.info(0).is_some());
        assert!(Font::new(pool.faces[0].0.clone(), pool.faces[0].1).is_some());
    }

    #[test]
    fn typst_assets_fonts_feature_is_on() {
        let pool = build_pool(&[]);
        assert!(
            !pool.faces.is_empty(),
            "typst-assets 字体没编进来（fonts feature 关了？）"
        );
        assert_eq!(missing_cjk_families(&pool.book), CJK_FAMILIES.to_vec());
    }

    #[test]
    fn same_font_dirs_reuse_the_pool() {
        let dirs = font_dirs();
        let a = font_pool(&dirs);
        let b = font_pool(&dirs);
        assert!(Arc::ptr_eq(&a, &b), "目录集相同却重建了字体池：83MB 白解析");
    }

    #[test]
    fn svg_pages_are_one_per_layout_page() {
        let source = "第一段。\n第二段。\n第三段。\n";
        let dirs = font_dirs();
        let out = compile_svg_pages(&request(source, &dirs)).unwrap();
        assert_eq!(
            out.output.len(),
            1,
            "三小段该在同一页：{}",
            out.output.len()
        );
        assert!(out.output[0].starts_with("<svg"), "{}", out.output[0]);
    }
}
