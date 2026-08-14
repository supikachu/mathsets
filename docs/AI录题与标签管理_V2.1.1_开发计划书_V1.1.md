# AI 录题与标签管理 V2.1.1（资料类型扩展版）开发计划书 V1.1

> 版本：V1.1（开发基线）｜基于 V1.0 评审意见修订：10 项🔴必改 + 10 项🟠建议 + 5 项🟡降级为技术决策，全部落位到具体字段/API/Worker/前端/验收 Case。
> 依据文档：《AI录题与标签管理_V2.1.1_资料类型扩展版.md》+《AI录题与标签管理V2_差异诊断与可行性分析》
> 分析基线：codegraph 索引（2026-08-02 构建，957 实体/6207 关系/37 社区）+ 2026-08-13 最新源码核实

## 〇、修订对照（评审意见 → 计划落点）

| 评审意见 | 落点 |
|---|---|
| ① 元数据职责未划分 | §三 元数据归属原则 |
| ② Mixed 人工拆分不完整 | §六.2 业务规则（整体解析后人工分组） |
| ③ "解析前拆/解析后拆"冲突 | §六.2 明确定死：先整体解析、后按题目分组 |
| ④ document_type 与 collection_type 重复 | §四 类型体系（两套职责不同 + 映射表） |
| ⑤ Paper/Collection 复用规则缺失 | §六.1 创建/复用规则 |
| ⑥ 历史 normalized hash 置 NULL 风险 | §八 Rust 离线回填 Job |
| ⑦ Worker 恢复不安全 | §七.3 租约/心跳/原子认领 |
| ⑧ 新旧 Tag Merge 双体系 | §六.3 旧 tags 合并冻结、统一走 knowledge_nodes canonical |
| ⑨ file_id 与 Schema 不一致 | §五 方案 A：documents.id 即文件实体 ID，无 file_id 列 |
| ⑩ school 是否作 Tag | §五 papers 增 school_name 正式列，school 实体后置 |
| ⑪ 题目分组 UI 缺失 | §十 F1 第 5 步 + 分组 API |
| ⑫ 来源查询缺 Question→Sources | §九 `GET /questions/{id}/sources` |
| ⑬ CollectionQuestion 题号约束 | §五 取消 (collection_id,question_no) 唯一 |
| ⑭ 取消后已成功题目 | §六.4 取消语义 |
| ⑮ Task 统计字段不足 | §五 ai_parse_tasks 新列全集 |
| ⑯ 基础数据一致性检查 | §九 `GET /admin/data-quality/summary` |
| ⑰ 分类 AI 输入太弱 | §七.2 四级 fallback |
| ⑱ 缺 other/自定义类型 | §四 other + type_label |
| ⑲ 需求→任务追踪矩阵 | §十二 |
| ⑳ P0 再拆子阶段 | §十一 P0-A/B/C/D |
| ㉑-㉔ PDF 引擎/TEXT/无 files 表/单 Worker | §十四 技术决策 TD-1~TD-4（doc2x/MinerU 纳入 TD-1 adapter 方案） |

## 一、目标与主线

把"AI 单题/图片解析"升级为完整闭环，主线定死如下（后续 DB/API/Worker/前端/QA 全部对齐此链）：

```text
                        Document（= 用户上传的文件实体）
                           │
                   AI 分类（推荐）+ 用户确认（最终决定权）
                           │
               ┌───────────┴───────────┐
               │                       │
        exam / mock_exam          非试卷资料
               │                       │
             Paper              QuestionCollection
               │                       │
         PaperQuestion          CollectionQuestion
               │                       │
               └───────────┬───────────┘
                           ↓
                      Question（hash 去重/复用）
                           ↓
                      Tag Resolver
                       ↙         ↘
                    已匹配      未匹配（不阻塞落库）
                      ↓           ↓
               正式标签关系   tag_candidates
                                  ↓
                          New / Alias / Merge / Reject
                                  ↓
                       canonical_id（不物理删除）
```

## 二、范围决策（已确认 + 本轮定死）

1. 分阶段交付，每阶段验收后进入下一阶段（P0-A/B/C/D → P1 → P2 backlog）。
2. Mixed PDF：**先整体解析题目，解析后用户人工分组**（按 question_ids 分组，不按页码）；自动边界识别进 P2 backlog。
3. 旧同步接口 `/ai/parse-image`、`/ai/parse-text`、`/ai/parse`、`/ai/parse/{id}` 删除；前端同步切换为异步任务队列（Markdown 本地粘贴模式不动）。
4. 新确认：Document→Task 为 1:N；Paper/Collection/Question 复用规则见 §六.1；取消语义见 §六.4。

## 三、元数据归属原则（解决"三个 subject 读哪个"）

| 实体 | 语义 | 保存字段 |
|---|---|---|
| **Document** | 用户实际上传的原始文件 | file_name、file_size、mime、page_count、document_type、type_label、source_type、sub_source_type、status、ai_classification、metadata（含 paper_meta 快照） |
| **Paper** | 具有"试卷"语义的题目集合 | title、year、stage、grade、subject、semester、region_province、region_city、school_name、source_type、sub_source_type、total_score、duration_minutes、document_id |
| **QuestionCollection** | 文件中一组题目的集合 | title、collection_type、type_label、source_type、subject、stage、grade、semester、chapter_id、metadata |

规则：**业务读取以 Paper/Collection 字段为准；document_type 只存在于 Document 层**；Document 的 source_type 描述文件来源，Collection/Paper 的 source_type 描述题目集来源（如 document_type=class_exercise + source_type=teacher_created）。

## 四、类型体系（两套枚举，职责不同）

- **DocumentType（文件整体是什么）**：exam、mock_exam、class_exercise、class_example、homework、preview_exercise、textbook_example、teaching_material、exercise_book、chapter_exercise、unit_exercise、special_training、wrong_question、mixed、unknown、**other**。TEXT + 后端白名单校验；other 时 `type_label` 必填（如"校本资料/竞赛资料"）。
- **CollectionType（这一组题是什么）**：class_exercise、class_example、homework、preview_exercise、textbook_example、teaching_material、exercise_book、chapter_exercise、unit_exercise、special_training、wrong_question、**other**（不含 exam/mock_exam/mixed/unknown——那是 Document 层概念）。
- **默认映射**：非 mixed 且非 exam/mock_exam 的 document_type 直接映射同名 collection_type（other→other+label）；mixed → 每个集合由用户各选 collection_type；exam/mock_exam → 建 Paper 不建集合。

## 五、数据模型（字段全集 + 约束）

**documents**（新表；方案 A：id 即文件实体 ID，**不设 file_id 列**）：
id UUID PK、creator_id FK users、file_name TEXT NOT NULL、file_size BIGINT、mime、page_count INT DEFAULT 1、document_type TEXT NULL（confirmed 前 NULL）、type_label TEXT NULL、title TEXT NULL、source_type/sub_source_type TEXT NULL、status TEXT DEFAULT 'uploaded'（uploaded/classifying/classified/confirmed/parsing/done/failed/cancelled）、ai_classification JSONB NULL（{document_type,title,confidence,reason,level,checked_pages}）、metadata JSONB DEFAULT '{}'、conversion_engine TEXT NULL（TD-1）、created_at/updated_at。索引：(creator_id, created_at DESC)、(status)。

**question_collections**（新表）：id、document_id FK CASCADE、creator_id FK、title TEXT NOT NULL、collection_type TEXT NOT NULL、type_label、source_type、subject、stage、grade、semester、chapter_id UUID NULL REFERENCES knowledge_nodes ON DELETE SET NULL、metadata JSONB、created_at/updated_at。部分唯一索引 (document_id, title)（同文档内幂等复用键，跨文档不复用）、索引 (creator_id)。

**collection_questions**（新表）：id、collection_id FK CASCADE、question_id FK CASCADE、question_no TEXT NULL（自由格式 1/1(1)/一、1，**不设唯一约束**）、display_order INT DEFAULT 0、section、score INT NULL、metadata JSONB、created_at。**UNIQUE(collection_id, question_id)**；索引 (question_id)。题号重复由 P1 数据质量检查报告，不阻塞写入。

**papers**（增列）：year INT、stage、semester、region_province、region_city、**school_name VARCHAR(200)**、source_type、sub_source_type、document_id UUID NULL FK documents ON DELETE SET NULL、metadata JSONB DEFAULT '{}'。复用键：document_id（同文档重跑幂等）+ 用户显式关联。

**paper_questions**（增列）：question_no TEXT NULL（不唯一）、display_order INT（回填 = sort_order）。**UNIQUE(paper_id, question_id)**；取消 V1.0 的 (paper_id,question_no) 唯一。

**questions**（增列）：content_hash TEXT、normalized_content_hash TEXT（均为 SHA-256 hex；见 §八 规范化算法）+ 索引 (normalized_content_hash)。

**ai_parse_tasks**（增列；Document 1:N，**不加唯一约束**）：document_id UUID NULL FK、paper_meta JSONB（输入快照：document_type/title/paper 元数据/collections 快照）、total_count/processed_count/success_count/failed_count/retry_count INT DEFAULT 0、current_page INT、total_pages INT、current_question_no TEXT、started_at TIMESTAMPTZ、completed_at TIMESTAMPTZ、last_error TEXT、progress JSONB DEFAULT '{}'（`idempotency_map: {question_index → question_id}`）、**locked_at TIMESTAMPTZ、worker_id TEXT、heartbeat_at TIMESTAMPTZ、cancel_requested_at TIMESTAMPTZ**。
状态机：pending → processing →（retrying ⇄ pending）→ success / partial_success / failed / cancelled；历史 completed 读出映射 success。`ALTER TYPE ai_task_status ADD VALUE` ×4 放独立迁移文件（同事务不使用新值）。

**knowledge_nodes**（P1 增列）：canonical_id UUID NULL FK、status TEXT DEFAULT 'active' CHECK(pending_review/active/merged/deprecated/rejected)、source TEXT DEFAULT 'system'；CHECK(canonical_id<>id)；部分索引 (canonical_id)。历史回填 status=active、source=system。
**tag_candidates**（P1 新表）：id、kind(chapter/knowledge/method)、raw_name、normalized_name、suggested_node_id、ai_confidence NUMERIC(5,4)、match_score NUMERIC(5,4)、source_task_id、source_question_id、status(pending/approved/rejected/merged)、reviewed_by、reviewed_at、created_at；幂等键 (source_task_id, source_question_id, normalized_name)。
**tag_merge_records**（P1 新表）：id、target_type('knowledge_node')、source_tag_id、target_tag_id、operator_id、operator_type、reason、created_at。

## 六、业务规则

### 6.1 创建/复用规则（定死）
- **Document**：每次上传 = 新 Document，永不复用。
- **Paper**：仅两种复用——(a) 同一 document_id 重跑（幂等）；(b) 用户 confirm 时显式选择"关联已有试卷"（paper_meta.paper_id）。其余一律新建。
- **QuestionCollection**：复用键仅 (document_id, title)（同文档重跑幂等）；跨文档同名资料一律新建，防 AI 误合并。
- **Question**：normalized_content_hash 精确命中 → 复用；近似重复不自动合并（P3）。
- **parse-task**：每次 POST 新建任务；若该 document 存在未终态任务返回 409 + existing_task_id（不静默复用）。

### 6.2 Mixed PDF 流程（定死：先整体解析、后按题目分组）
confirm 阶段（document_type=mixed）仅创建集合壳（每项 title+collection_type，可增删，与页码无关）；Worker 整体解析全部题目但不建 collection_questions；任务成功后前端进入**分组步骤**：题目列表（题号/题干/已归属集合）支持单选/范围多选 → 目标集合下拉 →"全部归入"便捷操作 → 调 `POST /collections/{id}/questions/batch`；未分组题目保留"未分组"标记，可稍后在 Collection 详情页补分。非 mixed 文档：Worker 自动把全部题目归入默认集合（映射自 document_type）。

### 6.3 标签合并体系统一
- 旧 `tags` 体系（core_competence/method/school 等）：`POST /tags/{id}/merge` **冻结**（保留现状兼容，不再扩展、不写新审计）。
- 新治理统一走 **knowledge_nodes → canonical_id + tag_merge_records**（不物理删除）。
- 题目维度学校标签（tags category=school）继续保留用于题目打标，与 Paper 级 school_name 不冲突。

### 6.4 取消语义（定死）
pending/processing/retrying 可取消 → 置 cancel_requested_at，worker 题间检查后置 **cancelled**；**已成功落库的题目全部保留**，success_count 如实反映；终态优先级：cancelled > failed > partial_success > success（即"取消过"最终就是 cancelled，即使已有部分成功）。

## 七、Worker 设计（P0-C）

### 7.1 阶段
- **Stage 0 原子认领**：`UPDATE ... SET status='processing', locked_at=NOW(), worker_id=$w, heartbeat_at=NOW(), started_at=COALESCE(started_at,NOW()) WHERE id=$1 AND status IN ('pending','retrying')`（SKIP LOCKED 出队）。
- **Stage 1**：读 document + paper_meta 快照，校验 document.status='confirmed'。
- **Stage 2 容器**：exam/mock_exam → 建/复用 Paper（§6.1）；非试卷 → 建/复用集合（幂等键 (document_id,title)）；无集合快照 → 建默认单集合。
- **Stage 3**：逐页 vision OCR（复用 batch prompt + post_process_batch 逐题隔离）→ **Stage 3b 跨页组装（文本模型，可选配置默认开）**：合并跨页续题、题号去重重排、display_order 连续化；失败降级为按页拼接+顺序编号 → 逐题：规范化→hash→复用/新建 Question（单题事务：question + paper_questions/collection_questions(question_no/display_order) + match_knowledge_nodes 落关系）→ 未匹配：P1 起写 tag_candidates（不阻塞）；P0 仅日志 → 更新 progress.idempotency_map、counters、current_page/current_question_no、heartbeat；**题间检查 cancel_requested_at**。
- **Stage 4 终态**：按 §6.4 优先级落 success/partial_success/failed/cancelled + completed_at。
- **错误分类**：NoApiKey/用户不存在 → failed（不可重试）；Upstream 5xx/429/Timeout/JSON 非法 → retry（retry_count+1 → retrying → pending，上限 2 次后 failed）；页面级失败不消耗 retry，计入 failed_count 走 partial_success。

### 7.2 分类 AI 多级 fallback（classify_document 服务端执行）
Level 1：文件名（text 模型）→ 置信 <0.6 → Level 2：文件名+第 1 页图（vision）→ <0.6 → Level 3：+前 3 页图（vision）→ <0.6 → **unknown**（前端强制用户选择）。最多 3 次 LLM 调用；ai_classification 记录 level/checked_pages/reason。

### 7.3 租约与恢复（多 Worker 安全）
- 租约 60s、心跳 20s（每页处理前后 refresh heartbeat_at）。
- 恢复规则：worker 每轮主循环执行 `UPDATE ai_parse_tasks SET status='pending', retry_count=retry_count+1, locked_at=NULL WHERE status='processing' AND heartbeat_at < NOW() - INTERVAL '120 seconds' AND retry_count < 2`；retry_count≥2 的僵尸任务 → failed。只有"超时且无心跳"才允许重新入队，杜绝双 Worker 并发处理（配合 Stage 0 的原子认领双保险）。

## 八、历史数据迁移（含 hash 回填方案）

- 迁移文件序列：`20260815000001_v211_task_status.sql`（仅 ADD VALUE ×4）→ `...02_documents.sql` → `...03_papers_questions.sql`（§五 全部列/表/索引，display_order 回填=sort_order，question_no 留 NULL）→ `...04_ai_parse_tasks.sql`（§五 新列）。
- **hash 回填（修正 V1.0 方案）**：规范化算法在 Rust 单点实现 `src/util/normalize.rs`（步骤：Unicode NFKC → 全角/半角统一 → 空白折叠 → 常见 LaTeX 空格（\, \; \quad）归一 → 行尾标点剥离 → 不 lower-case 以保数学语义）；content_hash = SHA-256(stem‖options‖answer‖analysis 规范化拼接)；normalized_content_hash = SHA-256(规范化 stem‖options‖answer)。**新增离线 Job `src/bin/backfill_question_hashes.rs`**（仿 import_trees.rs，幂等、分批、只回填 NULL 行）；迁移脚本仅做建列，不产生永久 NULL 缺口；Worker/创建接口对新数据即时计算。
- 历史兼容（文档 27 节 8 条）全部纳入验收：completed→success；老 paper_questions 无 question_no 前端回退 sort_order；老节点无 status/canonical 正常检索。

## 九、API 清单

| 变更 | 端点 |
|---|---|
| P0 新增 | `POST /ai/documents`（multipart file/pages[]，magic-number 校验）、`POST /ai/documents/{id}/classify`、`POST /ai/documents/{id}/confirm`、`GET /ai/documents`、`GET /ai/documents/{id}`、`POST /ai/parse-task`（存在未终态任务→409+existing_task_id）、`GET /ai/parse-task/{id}`（status/counters/current_page/current_question_no/结果关联）、`POST /ai/parse-task/{id}/cancel`、`GET /questions/{id}/sources`（统一来源视图：kind=paper/collection、title、type、question_no、display_order、document_id/title，现 /questions/{id}/papers 保留兼容）、`POST /collections/{id}/questions/batch`、`DELETE /collections/{id}/questions/{question_id}`、`GET /collections`、`GET /collections/{id}` |
| P1 新增 | `GET /admin/tag-candidates`、`GET /admin/tag-candidates/{id}`、`POST /admin/tag-candidates/{id}/approve`（body 分派 new_node[tree_id/parent_id/name]/alias[target_node_id]/merge[target_node_id]）、`/reject`、`POST /knowledge-nodes/{id}/merge`（递归 CTE 环检测→status=merged+canonical_id→关系迁移去重→写 merge_records）、`GET /admin/data-quality/summary`（孤儿关联/无题 Paper/canonical 环/重复题号/Candidate 超期）、`GET /tags/{id}/usage` |
| 修改 | `GET /questions` 增 year/semester/region/source_type/document_type/collection_id 过滤；`GET /papers` 元数据组合过滤；题目/试卷详情返回新元数据 |
| 冻结 | `POST /tags/{id}/merge`（仅兼容，不再扩展） |
| 删除 | `POST /ai/parse-image`、`POST /ai/parse-text`、`POST /ai/parse`、`GET /ai/parse/{id}`（前端调用与死代码 useAiParsePolling/aiTaskApi 旧实现一并清理） |

## 十、前端设计（F1）

AiRecognizeDialog 图片/PDF 分支重做为 5 步：
1. **选文件**：图片直接；PDF 走 TD-1 引擎（默认前端 pdfjs 渲染页图，上限 30 页超限提示截断）。
2. **上传**：`POST /ai/documents`（pages[]）→ 自动触发 classify。
3. **资料类型确认页**：AI 推荐卡（类型/置信度/理由/检测层级）；16 类单选 + other（自定义名）；exam/mock_exam → Paper 元数据表单（名称/年份/学段/年级/学科/学期/省/市/学校名/来源/子来源 + "关联已有试卷"下拉）；非试卷 → Collection 元数据（名称/学段/年级/学科/学期/章节[知识树级联]/来源）；mixed → 集合壳编辑器（增删、每项 title+type）；unknown → 强制选择。
4. **提交**：confirm → parse-task → 进度页（当前页/总页、当前题号、已处理/成功/失败、可取消）。
5. **结果**：非 mixed → 自动入默认集合，`emit('batch-parsed')` 进 QuestionEdit 多题工作台（携带 paper/collection 关联写入落库）；mixed → **分组步骤**（§6.2 UI）→ 完成后进工作台。
- 新建组件：TaskProgressPanel、DocumentTypeConfirmStep、QuestionGroupingStep；`useAiParsePolling` 重写适配新状态机。
- P1 新建：TagCandidateReview.vue（列表/详情/审核四分支）、CollectionList.vue / CollectionDetail.vue（来源链路展示 + 补分题目）、QuestionList 筛选扩展、KnowledgeTreeManagement 节点合并 UI（目标选择+原因+环错误提示）、TagManagement 不新增合并功能（冻结）。

## 十一、阶段拆分与任务编号

- **P0-A Document 基础**：M1（迁移 01/02/04）→ B1 documents 模型/上传/classify(多级 fallback)/confirm → F1 步骤 1-3 → T1（Case 6/7/12/13）。
- **P0-B Collection/Paper**：M1（迁移 03）→ B2 papers/collections 模型与 CRUD、复用规则、sources API → F1 步骤 3 分类型表单 → T2（Case 1/2/3/4）。
- **P0-C Worker**：B3 状态机/租约心跳/阶段 1-4/跨页组装/幂等/partial_success/cancel → F1 步骤 4-5（进度/分组） → T3（Case 8/9/10 + 幂等/恢复单测）。
- **P0-D 资料来源闭环**：B4 questions/{id}/sources、collection 详情链路、data-quality 基础项（孤儿关联/无题 Paper）→ F1 来源展示 → T4（Case 11）。
- **P1 标签治理 + 检索**：M2（knowledge_nodes 三列 + tag_candidates + tag_merge_records）→ B5 candidate 写入/审核/merge/环检测、tags 合并冻结、检索扩展、data-quality 扩展 → F2（审核页/合并 UI/筛选/Collection 页）→ T5（文档 31 节标签验收 + 检索组合用例）。
- **P2 backlog（不实施）**：Mixed 自动边界识别、章节边界识别、例题/练习/作业自动拆分、数据质量定时任务、embedding 召回、AI 同义判断、高级去重、School 实体化。

## 十二、需求→任务追踪矩阵

| 需求 | 阶段 | 数据 | API | Worker | 前端 | QA |
|---|---|---|---|---|---|---|
| Document 上传/分类/确认 | P0-A | M1 | B1 | — | F1 | T1 |
| 多级分类 fallback + unknown | P0-A | M1 | B1 | — | F1 | T1 |
| other/自定义类型 | P0-A | M1 | B1 | — | F1 | T1 |
| Paper 元数据/复用 | P0-B | M1 | B2 | W(阶段2) | F1 | T2 |
| QuestionCollection/CollectionQuestion | P0-B | M1 | B2 | W(阶段2) | F1 | T2 |
| Mixed 人工分组 | P0-B | M1 | B2(批量分组) | W(整体解析) | F1(分组步骤) | T2 |
| Question Hash 去重 + 历史回填 | P0-B | M1+Job | — | W(阶段3) | — | T2/T3 |
| Task 状态机/统计/1:N | P0-C | M1 | B3 | W(阶段0/4) | F1(进度) | T3 |
| 幂等/重试/partial_success/取消 | P0-C | M1 | B3 | W | F1 | T3 |
| 租约/心跳/恢复 | P0-C | M1 | — | W | — | T3 |
| 来源链路 questions/{id}/sources | P0-D | — | B4 | — | F1 | T4 |
| 基础数据一致性检查 | P0-D/P1 | — | B4/B5 | — | — | T4/T5 |
| Tag Candidate/审核四分支 | P1 | M2 | B5 | W(写 candidate) | F2 | T5 |
| Canonical 合并/环检测/merge_records | P1 | M2 | B5 | — | F2 | T5 |
| 旧 tags 合并冻结 | P1 | — | B5 | — | F2(冻结) | T5 |
| 资料类型/试卷元数据检索 | P1 | — | B5 | — | F2 | T5 |
| 历史兼容（8 条） | 全程 | 全程 | 全程 | 全程 | 全程 | 全程 |

## 十三、异常与一致性清单

LLM 超时/JSON 非法→retry≤2→failed；单页失败不连坐；单题失败→partial_success 且成功题可用；取消→保留已落库题目（§6.4）；僵尸任务→租约恢复；重试→idempotency_map+唯一索引+容器幂等键三重防护；hash 命中→复用；canonical 环/自指→拒绝；AI 低置信→unknown 强制选择；重复题号→不阻塞、P1 质量检查报告；文件安全（magic-number 复用、pages 按 document id 隔离、PDF 后端不解析）；历史数据升级后全部可读。

## 十四、技术决策（非需求约束，允许后续替换）

- **TD-1 PDF 转换引擎**：文档化三种方案，V2.1.1 默认 **A：前端 pdfjs 渲染页图**（零新依赖）；**B：doc2x API 后端解析**（PDF→文档数据流，需第三方 key）；**C：MinerU 本地引擎**（需部署 Python 模型服务，PDF→markdown/数据流）。B/C 均返回"文档数据流"，后续接入时只需实现同一转换 adapter 接口并写 documents.conversion_engine，**无需改 schema 与任务流程**。本轮按 A 实施；若验收前决定启用 doc2x/MinerU，需另行提供 key/部署并调整 P0-A 上传端（列为阶段一启动前可确认项）。
- **TD-2**：document_type/status/source 用 TEXT+白名单校验（文档建议 PG enum，等价实现）。
- **TD-3**：不新建 files 表；documents.id 即文件实体 ID（方案 A 已定死，无 file_id 字段）。
- **TD-4**：单 Worker 实例为本期部署形态；租约/心跳已按多 Worker 安全设计，横向扩展无需改代码。

## 十五、测试与验收

- **单元**：normalize 算法（含全角/半角/LaTeX 用例）、hash 幂等、类型白名单/映射、状态映射 completed→success、取消/终态优先级、环检测。
- **集成（tests/api.rs 真实 DB）**：documents 上传/分类 mock、confirm 校验分支（exam 缺 title 400、mixed 缺集合 400、other 缺 label 400）、parse-task 409 与幂等、cancel、租约恢复 SQL、worker 纯函数 `persist_task_questions` 直测（Paper/Collection/question_no/去重复用/partial_success 计数/取消保留）。
- **人工验收 Case**：文档 Case1-7（5 走人工分组）+ 新增 Case8 重复上传（新 document、hash 复用题目、集合新建）、Case9 取消保留已成功题目、Case10 重跑幂等（Question/关联数不增）、Case11 来源链路展示、Case12 分类 fallback 逐级升级、Case13 other 自定义类型、Case14 历史数据兼容（老 Paper/老题目/老任务/老标签）。

## 十六、风险与缓解

| 风险 | 缓解 |
|---|---|
| Worker 全量重写回归面大 | P0-A/B 先行落库能力；`persist_task_questions` 无 LLM 纯函数可直测；每子阶段验收 |
| LLM 输出字段增多出错 | 新字段全可选默认值；逐题隔离解析；跨页组装失败降级 |
| 双 Worker 并发/僵尸任务 | 租约+心跳+原子认领（§7.3） |
| 删除同步接口影响存量用户 | 前端本轮同步切换；Markdown 本地模式不变 |
| 多页 OCR 成本 | 页数上限 30、跨页组装可选配置、进度展示、可取消 |
| 历史 hash 一致性 | Rust 单点实现算法 + 离线 Job 回填（§八），SQL 不做第二套实现 |

## 十七、工作量与里程碑

| 阶段 | 后端 | 前端 | QA | 里程碑 |
|---|---|---|---|---|
| P0-A | 1.5-2 人日 | 1-1.5 人日 | 0.5 人日 | 上传→分类→确认闭环（Case 6/7/12/13） |
| P0-B | 1.5-2 人日 | 1 人日 | 0.5 人日 | Paper/Collection 落库（Case 1/2/3/4） |
| P0-C | 2-3 人日 | 1.5 人日 | 1 人日 | 异步录题全链路（Case 8/9/10） |
| P0-D | 0.5 人日 | 0.5 人日 | 0.5 人日 | 来源链路（Case 11） |
| P1 | 3-4 人日 | 3 人日 | 1 人日 | 标签治理闭环 + 检索验收 |
| P2 | — | — | — | backlog 单独立项 |

**实施顺序**：P0-A → 验收 → P0-B → 验收 → P0-C → 验收 → P0-D → 验收 → P1 → 验收。
