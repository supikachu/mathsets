---
name: Sync paper attrs to questions
overview: 在已实现的 OCR 先行 + 来源级联之上，把试卷信息（学段/学科/年级/学期/年份/省市区/来源/试卷关联）同步写入本批识别题目的表单属性与保存 payload，保证编辑页展示与落库一致。
todos:
  - id: map-helper
    content: questionSource.ts 增加 paper_meta → 题目表单字段映射（含 source_type / subject 归一）
    status: pending
  - id: apply-batch
    content: onAiSourceUpdated / handleBatchParsed：整批 questionList + form 覆盖属性与 paperIds
    status: pending
  - id: save-consistent
    content: 单题保存与全部保存前再 apply 一次，保证落库与侧栏一致
    status: pending
isProject: false
---

# 试卷信息同步到识别题目属性

## 现状缺口

[`SourceCascadeBar`](frontend/src/views/edit/components/SourceCascadeBar.vue) 已通过 `@source-updated` 把 `paper_meta` 传到 [`QuestionEdit.vue`](frontend/src/views/QuestionEdit.vue)。[`buildPayloadFromSource`](frontend/src/views/QuestionEdit.vue) 只在 **保存时的 metadata** 里覆盖部分字段（year/stage/grade 等），但：

- 未回写 `form` / `questionList[]` 快照上的同名字段，属性侧栏仍显示空或 AI 旧值
- `source_type` 未从级联大类/子类映射到题目「来源」字段
- `paperIds` 仅在 `create_paper && paper_id` 时更新；用户勾选「关联已有试卷」或后置建卷后，整批快照未统一挂卷
- 「全部保存」走快照循环，若快照本身未带卷属性，只能靠 `aiSourceState` 临时覆盖 metadata，编辑页预览与再次打开仍不一致

**默认策略（本计划采用）：** 来源条变更后，**本批 `questionList` 全部题目**（含当前 `form`）统一覆盖为同一套属性；不区分已保存/未保存。用户若要保留单题差异，可事后在编辑页改。

## 字段映射

从 `QuestionSourceState` / `paper_meta` → 题目快照与 form：

| 试卷信息 | 题目字段 |
|---|---|
| stage | `stage` |
| subject | `subject`（中文「数学」等 → 与 AttributeSidePanel 一致；若已是 `math`/`physics` 原样） |
| grade | `grade` |
| semester | `grade_semester`（`first`/`second`/`full_year`，与侧栏一致） |
| year | `year`（字符串） |
| region_province / region_city | `region_province` / `region_city` |
| source_category + source_kind | `source_type` 展示名（如「模拟题」）；`sub_source_type` 一模/二模 |
| create_paper + paper_id | `paperIds` / 保存时 `paper_ids` |
| school_name | 写入 metadata `school_name`（侧栏若无独立字段则仅 metadata） |

练习/其他大类：仍同步来源级联到 metadata；无 `paper_meta` 时不改卷关联（`paperIds = []`）。

## 实现

1. **公共映射**（[`frontend/src/utils/questionSource.ts`](frontend/src/utils/questionSource.ts)）  
   - `applySourceStateToQuestionFields(state): Partial<form 字段>`  
   - `sourceKind → source_type` 中文标签；subject 中文 ↔ 代码归一  

2. **[`QuestionEdit.onAiSourceUpdated`](frontend/src/views/QuestionEdit.vue)**  
   - 保存 `aiSourceState`  
   - 调用映射结果，写入当前 `form`  
   - 遍历 `questionList`，每题合并同一套属性字段（保留 stem/options/答案等）  
   - `create_paper`：有 `paper_id` 则 `paperIds = [id]`；否则清空  
   - `saveAiDraft()` 持久化  

3. **保存路径**  
   - `buildPayloadFromSource`：以快照字段为准，再用 `aiSourceState` 补缺（与现有逻辑对齐，避免双重冲突）  
   - `handleSave` / `handleSaveAllRecognized`：在组 payload 前若存在 `aiSourceState`，先对目标快照再 apply 一次，保证「全部保存」与单题保存一致  

4. **识别完成首批**  
   - `handleBatchParsed` 之后若已有 `aiSourceState`，立即对新建 `questionList` apply 一遍（避免分类条先于卡片填完、卡片后到却丢属性）

5. **不改后端 schema**  
   - 仍写 `questions.metadata` + `paper_ids`；confirm/建卷逻辑沿用现有 OCR 先行实现  

## 验收

- 选「试卷 → 模拟题」，填学年/学段/年级/省市区，开「创建试卷」并关联或新建后：点进任意卡片，属性侧栏字段与来源条一致，且 `paperIds` 非空（建卷成功后）  
- 「全部保存」后重新打开题：metadata 与关联试卷与填写一致  
- 改来源为大类「练习」：整批 `paperIds` 清空，来源字段变为练习子类名  

## 明确不做

- 不按「仅未保存题」分支同步  
- 不重做 AttributeSidePanel UI  
- 不把学校名强行做成侧栏必填控件（仅 metadata）
