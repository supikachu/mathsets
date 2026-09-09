# MathType 二进制转 WMF 矢量工具 (Windows 免安装绿色版)

这是一个专门针对 Windows 环境定制的高性能、免官方桌面安装的 MathType 二进制（`.bin` / `MTEF` 字节流）转 `.wmf` 矢量文件转换工具。

该工具不仅能够高保真生成矢量的 `.wmf` 公式图形，还能精确提取公式的**基线垂直偏移量（Baseline Offset，单位为 pt）**，无缝对接到上游排版引擎（如 Word、InDesign、WebKit、Typst 等）进行行内公式精准垂直对齐。

---

## 🌟 核心特性

1. **会话级免提权字体动态加载**：
   - 启动时自动调用 Windows GDI API `AddFontResourceExW(font_path, FR_PRIVATE, 0)`。
   - 动态加载 `mathtype_core/Fonts/` 目录下的所有 TrueType / OpenType 字体（包括 `MT Extra` 和 `Euclid` 家族字体）。
   - **完全不需要管理员权限（无需 UAC 提权）**，不污染系统注册表，进程退出时自动卸载。

2. **多源输入格式自适应解析**：
   - **OLE 复合文档 (`.bin` / CFB)**：自动识别 `\xD0\xCF\x11\xE0\xA1\xB1\x1A\xE1` 魔数，并提取 `Equation Native` 数据流。
   - **Equation Native 流**：自动解析并剥离 28 字节 OLE 头部（`0x001C`）。
   - **原始 MTEF 字节流**：自动识别 MTEF v1~v5 版本头。
   - 上游传入任意一种形式（文件或 Base64），均能无缝自动解包。

3. **双重互证的基线（Baseline）偏移量计算**：
   - **主通道**：调用 `MT6.dll` 的 `MTGetLastDimension(mtdimBASELINE = 3)`，以 $1/32 \text{ pt}$ 为精度单位换算公式基线偏移（从公式外框底边缘算起）。
   - **校准通道**：直接解析生成的 WMF 元文件中 `META_ESCAPE (0x0626)` 的 `MFCOMMENT (15)` MathType 私有注释段，将 16 分之一点反解校验，实现 100% 精度与双重互信。

4. **高可用容错与 OLE 剪贴板双层降级机制**：
   - **第一层（直接内存转换）**：调用 `MTXFormEqn(src=MtxfmLocal, dst=MtxfmFile)` 直接生成文件。
   - **第二层降级（剪贴板输入转文件）**：若直接内存转换失败，自动将 MTEF 注册到 Windows 剪贴板（`"MathType EF"`），调用 `MTXFormEqn(src=MtxfmClipboard, dst=MtxfmFile)`。
   - **第三层降级（全剪贴板链路与 GDI 拓扑重构）**：若文件写入被限制，转换结果输出至剪贴板 `CF_METAFILEPICT`，通过 Windows GDI `GetMetaFileBitsEx` 获取图元，并自动合成为符合 Aldus Placeable Metafile（22 字节标准 APM 头，带校验和）的规范 `.wmf` 文件。
   - **死锁自愈机制**：内置会话管理器，能自动发现并清理卡死或无响应的僵尸 `MathType.exe` 进程并重新协商握手连接。

5. **单线程 STA 任务队列（Thread-Affine Architecture）**：
   - MathType 的 COM / DDE 机制严格依赖单线程公寓（STA）和主线程窗口消息泵。
   - 内部设计了专门的 STA 工作线程队列，无论 HTTP 请求在哪个并发线程池触发，均通过内存队列由绑定线程原子执行，彻底杜绝跨线程 DDE 死锁。

---

## 📁 目录架构说明

```text
tools/
├── mathtype_core/            # 抽取的绿色 MathType 运行时资产目录
│   ├── MT6.dll               # 64位转换核心动态链接库
│   ├── MathType.exe          # OLE 公式服务端程序
│   ├── MathTypeLib.exe       # 公式支持库
│   ├── Toolbar.eql           # 工具栏配置
│   └── Fonts/                # 必备字体库 (MT Extra.ttf, Euclid 字体家族)
│       ├── mtextra.ttf
│       ├── euclid.ttf
│       └── ...
├── mt_converter/             # 转换工具 Python 核心模块
│   ├── __init__.py
│   ├── core.py               # GDI字体动态挂载、MT6 C-API绑定与剪贴板容错链路
│   ├── mtef_parser.py        # CFB / Equation Native / MTEF 多格式自适应解析器
│   ├── service.py            # FastAPI & 原生 http.server 双模轻量 HTTP 微服务
│   └── cli.py                # 命令行客户端 (单文件、批处理、JSON输出)
├── main.py                   # 程序统一入口点
├── build.py                  # PyInstaller 自动化单文件打包脚本
├── requirements.txt          # Python 依赖清单
└── test_suite.py             # 单元与完整集成验证测试套件
```

---

## 🚀 使用指南

### 交付方案 A：轻量 HTTP 微服务（推荐）

#### 启动服务
默认监听本地 `127.0.0.1:8099`：
```powershell
python main.py --serve --port 8099
```
若已打包为可执行文件：
```powershell
mt_converter.exe --serve --port 8099
```

#### 接口 1：健康检查与环境探活
- **URL**：`GET /health` 或 `GET /`
- **响应示例**：
```json
{
  "status": "ok",
  "mathtype_core": "C:\\Users\\pikachu\\Desktop\\tools\\mathtype_core",
  "fonts_count": 23
}
```

#### 接口 2：公式转换 (`POST /convert`)
- **URL**：`POST http://127.0.0.1:8099/convert`
- **请求头**：`Content-Type: application/json`
- **请求体**：
```json
{
  "bin_base64": "0M/R4KGxGuEAAAAAAAAAAAAAAAAAAAAAPgADAP7/..."
}
```
- **成功响应体**：
```json
{
  "wmf_base64": "183GmgAAAAAAAIAB4AMACQAAAABxXAEACQAAAxIC...",
  "baseline_offset_pt": 12.0,
  "width_pt": 12.0,
  "height_pt": 31.0,
  "code": 0,
  "method": "direct_mt6_dll",
  "error": null
}
```
- **失败响应体**：
```json
{
  "wmf_base64": "",
  "baseline_offset_pt": 0.0,
  "width_pt": 0.0,
  "height_pt": 0.0,
  "code": 1,
  "method": null,
  "error": "Failed to parse OLE compound document..."
}
```

#### 排版对接建议：
上游系统（如 HTML/CSS、InDesign、Docx）渲染行内公式时，基线对齐公式通常如下：
- CSS 行内垂直对齐：`vertical-align: calc(-1 * (height_pt - baseline_offset_pt) * 1pt)` 或根据基线锚点偏移。

---

### 交付方案 B：CLI 控制台命令行工具

#### 1. 单文件转换并打印基线
```powershell
python main.py --input formula.bin --output formula.wmf --print-baseline
```
输出：
```text
[SUCCESS] Converted 'formula.bin' -> 'formula.wmf' (1082 bytes)
baseline_offset_pt: 12.0
```

#### 2. JSON 格式化输出（供上游进程通过标准输出捕获）
```powershell
python main.py --input formula.bin --output formula.wmf --json
```
输出：
```json
{
  "input": "formula.bin",
  "output": "formula.wmf",
  "baseline_offset_pt": 12.0,
  "width_pt": 12.0,
  "height_pt": 31.0,
  "method": "direct_mt6_dll",
  "code": 0
}
```

#### 3. 批量文件夹转换
```powershell
python main.py --batch-dir ./input_bins --output-dir ./output_wmfs
```
输出：
```text
[*] Batch converting 7 files to ./output_wmfs ...
  [OK] 01-frac.wmf | baseline: 12.0 pt
  [OK] 02-sqrt.wmf | baseline: 4.0 pt
  [OK] 03-fence.wmf | baseline: 15.0 pt
  [OK] 04-inline.wmf | baseline: 3.0 pt
[*] Completed: 7/7 converted successfully.
```

---

## 📦 打包为免安装单文件 EXE

工具内置了自动化构建脚本 `build.py`：

```powershell
python build.py
```

构建脚本将自动完成：
1. 收集 FastAPI、Uvicorn 依赖与动态协议驱动。
2. 通过 PyInstaller 打包生成独立的单个可执行文件 `dist/mt_converter.exe`。
3. 在 `dist/mt_converter_portable/` 下生成开箱即用的绿色发布包，将 `mt_converter.exe` 与 `mathtype_core/` 放置在一起。

交付给最终用户时，只需直接分发 `mt_converter_portable/` 目录即可，无需目标机器安装 Python、Visual Studio 或 MathType 官方客户端。
