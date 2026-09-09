# MathType 公式资产接入题库（分阶段整合说明）

## 架构

```text
录题 create/update/submit → 登记 question_math_assets（pending）并后台转换
                 → Formula Worker 轮询补齐失败/过期：
                      LaTeX → MathML（Rust）
                      MathML → ole.bin（MATHTYPE_OLE_CLI）
                      一题一批 POST /convert_batch → WMF+baseline
导出 options.docx_math = "mathtype" → 嵌入 Equation.DSMT4
         缺省 / "omml" → 现有 OMML 路径
```

资产按 **题目 + field + ordinal** 关联（非全局 hash）。提交审核默认不等待 `ready`（`MATHTYPE_REQUIRE_ON_SUBMIT=1` 才同步硬门槛）。

## 启动流程（重要）

**题库主进程不会自动拉起 `mt_converter.exe`。**  
`cargo run` / 启动整站只会起 Rust 侧的 Formula Worker（HTTP **客户端**），它假定转换微服务已经在监听。

推荐顺序（本机 Windows 开发）：

```text
① 准备便携包
   tools/mt_converter_portable/
     mt_converter.exe      （本地，gitignore）
     mathtype_core/        （含 MT6.dll + Fonts，本地）

② 启动 MathType WMF 微服务（单独终端，常驻）
   cd tools\mt_converter_portable
   .\mt_converter.exe --serve --port 8099
   探活：浏览器或 curl 访问 http://127.0.0.1:8099/health

③（可选）确认 MathML→bin CLI 已构建
   cd tools\mathtype-ole
   dotnet build MathTypeOle.sln -c Release
   CLI 路径示例：
   tools\mathtype-ole\MathTypeOle.Cli\bin\Release\net472\MathTypeOle.Cli.exe

④ 配置 .env（见下表）后启动题库
   cargo run
   # 或你们平时的前后端一键脚本
```

对照关系：

| 进程 | 谁启动 | 做什么 |
|------|--------|--------|
| `mt_converter.exe --serve` | **人工 / 脚本单独起** | bin/MTEF → WMF + baseline（STA） |
| `MathTypeOle.Cli.exe` | 按需由题库子进程调用 | MathML → ole.bin |
| `mathset`（`cargo run`） | 项目主启动 | 录题、轮询 pending、调上述 URL/CLI、导出 |

未配置 `MATHTYPE_CONVERT_URL` / `MATHTYPE_OLE_CLI` 时：主站仍可启动；公式只登记槽位、不转 WMF；Word 默认仍走 OMML。

生产建议：转换服务跑在独立 Windows Worker，主站用 `MATHTYPE_CONVERT_URL` 指向该机；Linux 主站不要本机硬起 exe。

## P0 工具就位

1. 覆盖 `tools/mt_converter_portable/mt_converter.exe` + 本地 `mathtype_core/`
2. 按上一节 **②** 启动 `--serve`
3. 构建 `tools/mathtype-ole` Release CLI（MathML→bin）

## 环境变量

| 变量 | 说明 |
|------|------|
| `MATHTYPE_CONVERT_URL` | 例 `http://127.0.0.1:8099` |
| `MATHTYPE_OLE_CLI` | `MathTypeOle.Cli.exe` 绝对路径 |
| `MATHTYPE_REQUIRE_ON_SUBMIT` | `1` 时提交审核**同步**等全部公式 `ready`（慢，易超时；默认关，提交只登记槽位并后台转） |
| `MATHTYPE_CONVERT_TIMEOUT_SECS` | 默认 120 |

未配置时：仅登记槽位，不阻断录题；Word 默认仍走 OMML。

## 迁移

```bash
sqlx migrate run
# 或你们现有的 migrate 流程
```

表：`question_math_assets`

## 导出

`POST /api/v1/export/docx` 请求体：

```json
{
  "options": {
    "docx_math": "mathtype"
  }
}
```

缺资产的公式自动降级 OMML。

## 已知边界

- ole.bin / WMF 转换需 **Windows Worker**
- **整站启动 ≠ 自动启动 converter**（见「启动流程」）
- 题干内嵌选项被装配器拆出时，stem 公式序号可能与资产不完全对齐；优先保证 `options` JSONB 独立存放
- `mathtype_core` / exe **不进 Git**

## 故障排查：`access violation reading 0x10`

症状：`mt_converter --serve` 日志里 Direct / Clipboard 三层全部 `access violation`，题库 Formula Worker 同步失败。

已验证结论（本机）：

| 探测 | 结果 |
|------|------|
| `GET /health` | 正常（core 路径对、fonts_count=23） |
| **新进程** CLI：`mt_converter.exe -i 已知好.bin -o out.wmf` | **成功**（`direct_mt6_dll`） |
| **旧的** `--serve` 对**同一** bin 调 `POST /convert` | **失败**（AV） |
| 杀掉全部 `mt_converter` 后只起 **一个** `--serve`，再 `POST /convert` / `/convert_batch` | **成功** |

根因通常是：

1. **同时开了两个独立的 `--serve`**（任务管理器里两个无父子关系的 `mt_converter`）抢 MT6 / 剪贴板；或  
2. 某次 AV 后 **长驻服务会话半死**，health 仍 ok，但 `MTXFormEqn` 必崩。

处理：

```powershell
# 结束全部转换相关进程（不要留第二个 --serve）
Get-Process mt_converter,MathType,MathTypeLib -ErrorAction SilentlyContinue | Stop-Process -Force

cd C:\Users\pikachu\Desktop\mathset\tools\mt_converter_portable
.\mt_converter.exe --serve --port 8099

# 冒烟（可选）
Invoke-RestMethod http://127.0.0.1:8099/health
```

说明：PyInstaller 单文件会有 **父子各一个** `mt_converter.exe`（子进程 Parent=父），这是正常的；异常的是 **两个互不隶属的 serve**。
