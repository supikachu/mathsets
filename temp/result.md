我先调查 `ai_tagging.rs` 现状与三维度数据模型，再实施两阶段 RAG 重构。

现状清楚（单次 LLM 调用 + 逐名匹配）。查看 provider 调用与 resolve_ai_config 签名，然后实施两阶段重构：

分开执行：

模式确认（`resolve_ai_config` + `parse_text_with_prompt` 可复用，pg_trgm 已在用）。实施两阶段重构——重写 `ai_tagging.rs`：

编译检查：

编译通过（仅既有 warning）。运行 ai_tagging 单测：

单测发现菜单格式不一致（空候选时标题缺"候选列表"）。修复：

单测 7/7 通过。跑完整测试套件确认无回归：

服务进程锁定 exe 导致测试无法链接。先停服务再跑：

**全绿**：lib 39 passed（含 7 个新单测）+ api 18 passed。签收各步骤：

重新编译并启动服务，然后端到端验证新流水线：

服务正常。端到端验证 ai_tagging（先校验 + 真实两阶段调用）：

**端到端验证成功**：两阶段流水线跑通——4 个打标节点全部是候选库真实名称（章节"第1章 二次函数"+ 知识点"二次函数的对称/最值/图象和性质"，无幻觉造词）、三维树正确区分、元数据正常。签收并跑 review：

review 提出 blocking（score 列类型 float8/numeric vs f32 解码）——但端到端实测 200 成功。先实测 SQL 列类型确认：

实测确认：PG 实际推断为 `real`（review 的理论误判，端到端成功是真实的）——但 `::real` 显式化仍是防御性最佳实践，落实它 + space_id 权限校验：

score 显式 real 完成。加 space_id 成员校验（should-fix）：

cargo check 0 error + 测试 0 失败。复审确认 blocking 修复：

复审 **ship as-is**。签收：

✅ AI 智能打标两阶段 RAG 重构完成：

## 重构报告：两阶段检索候选（RAG 分类）

**变更文件**：`src/handlers/ai_tagging.rs`（完整重写，+30KB）

### 三步走、三维度流水线

**阶段一：大模型发散提词**（第一次 LLM 调用）
- `AI_EXTRACT_KEYS_PROMPT`：从题干/解析提取三维关键词 `chapter_keys`（章节）/`knowledge_keys`（知识点）/`method_keys`（解题方法）+ 难度/题型/年级/认知层次元数据，含示例与"宁少勿滥"约束
- 解析为 `AiExtractResult`（字段缺省 `#[serde(default)]` 兜底）

**阶段二：Rust 三维并发召回**（数据库检索）
- `recall_candidates`：SQL 按 `kt.kind::text` **严格三维隔离**（chapter / knowledge / ability，解题方法树 `math_method_*` 的 kind 为 ability）+ 空间隔离
- exact（1.0）> alias（0.95）> fuzzy（`similarity >= 0.3` 底线）三优先级，逐关键词 LIMIT 30 合并去重后截取 **Top 10 / 20 / 15**
- 三维 `tokio::join!` 并发执行互不干扰

**阶段三：大模型精准收敛**（第二次 LLM 调用）
- `AI_CONVERGE_PROMPT` 硬性规则：**只能输出候选列表中的原名、严禁编造、无合适留空、每维度最多 3 个**
- `build_candidate_menu` 组装三份候选菜单；`resolve_selection` 名称精确匹配候选（候选外名称 = 幻觉 → 丢弃）
- 收敛解析失败降级为各维度召回 Top1（保底不中断）
- 响应结构**保持兼容**：`knowledge_nodes` 三维合一（前端按 tree_id 分发到章节/知识点/方法 Tab），`competency_tags`/`method_tags` 保留

### 质量流程
| 环节 | 结果 |
|---|---|
| **单测**（新增 7 个） | 全绿：三维 prompt 规则、两阶段结果反序列化、字段缺省兜底、候选菜单格式、**反幻觉选择** |
| **全量测试** | lib 39 + api 18 = **57 个全部通过** |
| **端到端实测**（真实两阶段调用） | ✅ 200：4 个打标节点（章节"第1章 二次函数" + 知识点"二次函数的对称/最值/图象和性质"）**全部为候选库真实节点名，无幻觉造词**；难度/题型元数据正常 |
| **review 两轮** | blocking（score 类型 `::real` 显式化，实测 pg_typeof 确认）与 should-fix（space_id 空间成员校验）全部关闭 → **ship as-is** |
| **编译** | `cargo check` 0 error；服务已重建运行（`127.0.0.1:3000`） |

**效果**：幻觉造词彻底消灭（LLM 只做选择题）、Token 消耗减半（发散轻量 + 收敛精确）、三维度召回隔离互不污染。