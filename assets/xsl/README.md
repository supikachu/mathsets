# `assets/xsl/` — OMML 黄金快照的参考样式表

本目录只放一个文件：**`MML2OMML.XSL`**（微软官方 MathML → Office Math ML 样式表）。

它不入库（见根目录 `.gitignore` 的 `/assets/xsl/*`），需要由每位开发者从本机 Office 拷贝一份。
理由：该文件是微软随 Office 分发的许可物料，仓库里放二进制/源码拷贝有许可风险；而**由它生成的输出**
是事实性数据，可以放心入库 —— 这就是实施计划修订 R2 的取舍。

## 放进来

```powershell
copy "C:\Program Files\Microsoft Office\root\Office16\MML2OMML.XSL" assets\xsl\MML2OMML.XSL
```

其它常见位置（脚本会自动探测，无需手动拷贝也能跑）：

- `C:\Program Files\Microsoft Office\root\Office16\`（Office 365 / 2016+ C2R）
- `C:\Program Files (x86)\Microsoft Office\root\Office16\`
- `C:\Program Files\Microsoft Office\Office\`

也可以用 `--xsl <路径>` 或环境变量 `MML2OMML_XSL` 直接指定，不必放进本目录。

## 用来做什么

**只在开发期用，运行时绝不执行**（服务端不引入 libxslt 之类的原生依赖，OMML 由
`src/export/math/omml.rs` 的 Rust 递归下降转换器生成）。

```bash
# 生成 / 再生成全部固件
python scripts/gen_omml_snapshots.py

# 只校验现有固件与 XSL 输出是否一致（CI / 提交前）
python scripts/gen_omml_snapshots.py --check

# 单个用例
python scripts/gen_omml_snapshots.py --case nary_sum
```

依赖：`python` + `lxml`（`pip install lxml`）。仓库里若装了 `xsltproc` 也可手工等价生成，
但脚本走 lxml，跨平台一致。

## 输入输出的对应关系

| 路径 | 角色 | 是否入库 |
| --- | --- | --- |
| `tests/snapshots/cases/<名>.mathml` | Presentation MathML 用例（首行注释记录其 LaTeX 来源） | ✅ |
| `tests/snapshots/<名>.omml` | 官方 XSL 对该用例的输出，即黄金快照固件 | ✅ |
| `assets/xsl/MML2OMML.XSL` | 生成固件用的样式表本体 | ❌（本地拷入） |

用例覆盖 §5.3 映射表的每一行：分式、无横线分式（`linethickness="0"`）、根号 / 高次根、上下标、
∑∫∏ 的 n-ary 上下限、矩阵、cases、定界符、`mfenced`/`menclose`、重音与横线、`mathvariant`
家族、中文 `mtext`、间距、以及 crate 与转换器都不认的构造（`mmultiscripts`/`merror`）。

## 改了 XSL 会怎样

不同 Office 版本的 `MML2OMML.XSL` 输出可能有差异（属性顺序、`m:rPr` 细节）。`omml.rs` 的测试
在比对前做三步规范化（命名空间前缀统一 → 属性排序 → 空白归一），版本间的小差异不影响断言；
真出现语义差异时，`--check` 会列出漂移的固件，按提示重新生成即可 —— **不要手工编辑 `.omml` 固件**。
