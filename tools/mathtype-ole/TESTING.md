# MathType OLE 测试手册

在 Windows **x64** 上验证 `tools/mathtype-ole`：

1. MathML → MathType `ole.bin`（CFB / Equation.DSMT4）
2. MathML / MTEF / `ole.bin` → **真 MathType WMF**（`MT6.dll` / `MTXFormEqn`）

**环境：** Windows + .NET SDK 8+ + net472；进程为 **x64**（加载 `MathType\System\64\MT6.dll`）。  
**ole.bin：** 不需要 MathType。  
**WMF：** 需要本机安装 MathType。  
**仍未覆盖：** Word OOXML `w:object` 装配（后续导出阶段）。

---

## 1. 构建

```powershell
cd c:\Users\pikachu\Desktop\mathset\tools\mathtype-ole
dotnet build MathTypeOle.sln -c Release
```

期望：`Build succeeded`，产物：

```text
MathTypeOle.Cli\bin\Release\net472\MathTypeOle.Cli.exe   # x64
MathTypeOle.Core\bin\Release\net472\MathTypeOle.Core.dll
```

---

## 2. 快速冒烟 — ole.bin（必做）

```powershell
$cli = ".\MathTypeOle.Cli\bin\Release\net472\MathTypeOle.Cli.exe"
& $cli .\samples\01-frac.mathml.xml .\samples\out\01-frac.bin --verify
```

| 检查 | 期望 |
|------|------|
| 退出码 | `0` |
| 文件头 | `D0 CF 11 E0` |
| 体积 | ≥ 512 字节（样例约 3584） |

```powershell
$b = [IO.File]::ReadAllBytes(".\samples\out\01-frac.bin")
"{0:X2} {1:X2} {2:X2} {3:X2}  len={4}" -f $b[0],$b[1],$b[2],$b[3],$b.Length
```

---

## 3. 快速冒烟 — WMF（必做）

优先使用 `tools/mt_converter_portable`（带 baseline）；若无便携包则回退系统 MT6。

```powershell
$cli = ".\MathTypeOle.Cli\bin\Release\net472\MathTypeOle.Cli.exe"
New-Item -ItemType Directory -Force -Path .\samples\out | Out-Null
& $cli .\samples\01-frac.mathml.xml .\samples\out\01-frac.wmf --wmf
& $cli --from-ole .\samples\out\01-frac.bin -o .\samples\out\01-from-ole.wmf
# 期望控制台含 engine=portable（若便携包在侧）
```

或一条龙脚本：

```powershell
.\scripts\mathml-to-bin-wmf.ps1 -MathMlPath .\samples\01-frac.mathml.xml -OutDir .\samples\out
```

| 检查 | 期望 |
|------|------|
| 退出码 | `0` |
| 控制台 | `(WMF)` |
| 文件头 | Placeable WMF key **`D7 CD C6 9A`** |
| 体积 | 通常数百～数 KB（分数样例约 1082） |

```powershell
$b = [IO.File]::ReadAllBytes(".\samples\out\01-frac.wmf")
"{0:X2} {1:X2} {2:X2} {3:X2}  len={4}" -f $b[0],$b[1],$b[2],$b[3],$b.Length
# 期望: D7 CD C6 9A  len=…
```

可用画图 / Word「插入 → 图片」打开 `.wmf`，观感应接近 MathType 编辑器，而不是自研渲染。

本机 DLL 默认：

- `C:\Program Files (x86)\MathType\System\64\MT6.dll`
- 可用环境变量 `MATHTYPE_MT6_DIR` / `MATHTYPE_MT6_DLL` 覆盖

---

## 4. 样例矩阵

```powershell
$cli = ".\MathTypeOle.Cli\bin\Release\net472\MathTypeOle.Cli.exe"
New-Item -ItemType Directory -Force -Path .\samples\out | Out-Null

# bin
& $cli .\samples\01-frac.mathml.xml      .\samples\out\01-frac.bin --verify
& $cli .\samples\02-sqrt-sup.mathml.xml  .\samples\out\02-sqrt-sup.bin --verify
& $cli .\samples\03-fence.mathml.xml     .\samples\out\03-fence.bin --verify
& $cli .\samples\04-inline.mathml.xml    .\samples\out\04-inline.bin --inline --verify
& $cli .\samples\05-cases-onesided.mathml.xml .\samples\out\05-cases-onesided.bin --verify
& $cli .\samples\01-frac.mathml.xml      .\samples\out\01-frac-14pt.bin --font-size 14 --verify

# wmf
& $cli .\samples\01-frac.mathml.xml      .\samples\out\01-frac.wmf --wmf
& $cli .\samples\02-sqrt-sup.mathml.xml  .\samples\out\02-sqrt-sup.wmf --wmf
& $cli .\samples\03-fence.mathml.xml     .\samples\out\03-fence.wmf --wmf
& $cli .\samples\04-inline.mathml.xml    .\samples\out\04-inline.wmf --inline --wmf

# both + from-ole
& $cli .\samples\01-frac.mathml.xml .\samples\out\01-both.bin --bin --wmf
& $cli --from-ole .\samples\out\01-both.bin -o .\samples\out\01-from-ole.wmf
```

全部退出码应为 `0`。

---

## 5. Demo DOCX（含公式，需 MathType + Word）

生成一份可双击打开的 Word 文档（内嵌 4 个样例公式的 `ole.bin` + placeable WMF）：

```powershell
$cli = ".\MathTypeOle.Cli\bin\Release\net472\MathTypeOle.Cli.exe"
& $cli --demo-docx -o .\samples\out\mathtype-demo.docx
```

期望：

| 检查 | 期望 |
|------|------|
| 退出码 | `0` |
| 包内部件 | `word/embeddings/oleObject{1-4}.bin` + `word/media/image{1-4}.wmf` |
| `document.xml` | 含 `ProgID="Equation.DSMT4"` 与 `w:object` |
| 手工 | 用 Word 打开 → 预览像 MathType → **双击**公式进入 MathType |

产物路径：`tools/mathtype-ole/samples/out/mathtype-demo.docx`

---

## 6. 常见失败

| 现象 | 可能原因 |
|------|----------|
| `LoadLibrary failed` / 位数不匹配 | 未用 x64 构建，或指到了 `System\32\MT6.dll` |
| `MTAPIConnect` / `mtCANT_RUN` | MathType 未装好、授权/首次启动问题 |
| `MTXFormEqn` / `mtNOT_EQUATION` | MTEF 损坏或非 v5 |
| `MT6.dll not found` | 安装路径不同；设 `MATHTYPE_MT6_DIR` |
| ole.bin 头不是 `D0CF11E0` | 写错输出或转换失败 |
| Word 打开 DOCX 但公式是红 X / 空白 | WMF 关系损坏，或未装 MathType（预览仍应显示 WMF） |
| 双击无进 MathType | ProgID/CLSID 或 bin 无效；或本机未注册 Equation.DSMT4 |

---

## 7. 测试通过清单

- [ ] `dotnet build -c Release`（x64）成功  
- [ ] `01-frac.bin`：头 `D0 CF 11 E0`，`--verify` OK  
- [ ] `01-frac.wmf`：头 `D7 CD C6 9A`，`(WMF)`  
- [ ] `--from-ole` 再出一份 WMF OK  
- [ ] 样例 02/03/04 WMF 退出码 0  
- [ ] `--demo-docx` 生成 `mathtype-demo.docx`  
- [ ]（手工）Word 打开 demo，预览正常，双击进 MathType  
