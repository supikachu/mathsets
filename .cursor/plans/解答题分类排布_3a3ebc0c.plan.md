---
name: 解答题分类排布
overview: 解答题编辑与预览改为按「题干 / 答案 / 解析」三类排布，数据仍挂在现有 parts 树上。单问计算题保持现在的无标签三块；有小问时题干区展示总前提+整棵问树，答案/解析区只列叶子并使用 (1)、(2)(i) 路径编号。
todos:
  - id: path-label
    content: questionParts.ts 增加 leafPathLabel；简单树仍无编号
    status: completed
  - id: edit-grouped
    content: EditFormSolution 拆成题干 / 答案 / 解析三区，绑同一棵 parts
    status: completed
  - id: preview-grouped
    content: "QuestionStructureView grouped +\_section；LivePreview / Detail / List / Paper 三段对齐"
    status: completed
isProject: false
---

# 解答题按题干/答案/解析分类排布

数据模型不变：`questions.structure.parts` 仍是 SSOT（分支只有局部题干，叶子才有 `answer` / `analyses`）。只改左栏录入和各处预览的**视觉分组**。

## ASCII 界面（有小问，含一层嵌套）

左栏录入：

```
题干
┌──────────────────────────────────────────┐
│ [总前提：若 a,b,c 为 △ABC 的三边长。]     │  ← 现有 form.stem，不进本组件
├──────────────────────────────────────────┤
│ (1)  [证明：… < 2]           [+子问][x]  │
│ (2)  [若 C 为直角…]          [+子问][x]  │
│      (i)  [求周长最小值]        [同级][x]│
│      (ii) [……]                  [同级][x]│
│ [+ 添加大问]                             │
└──────────────────────────────────────────┘

答案
┌──────────────────────────────────────────┐
│ (1)     [123455                    ]     │
│ (2)(i)  [09887                     ]     │
│ (2)(ii) [                          ]     │
└──────────────────────────────────────────┘

解析
┌──────────────────────────────────────────┐
│ (1)     解法一  [abcdfefdf         ]     │
│         [+ 添加新解法]                   │
│ (2)(i)  解法一  [45677886…         ]     │
│ (2)(ii) 解法一  [                  ]     │
│         ☑ 无需解析                       │
└──────────────────────────────────────────┘
```

右栏预览：

```
解答题  ★★★
────────────────────────────────
若 a, b, c 为 △ABC 的三边长。
(1) 证明: a/(b+c)+… < 2
(2) 若 C 为直角…
    (i)  求周长最小值
    (ii) ……

答案
  (1)     123455
  (2)(i)  09887
  (2)(ii) —

解析
  (1)     [解法一|解法二]  abcdfefdf
  (2)(i)  45677886…
  (2)(ii) —
```

单问计算题（`isSimpleTree`：仅 1 个空 stem 叶子）仍无 (1) 外壳：题干只有总前提，答案/解析各一块，与改嵌套前一致。点「增加小问」后才出现编号行。

选中联动：点题干某问、或点答案/解析对应行，共用现有 `expandedPartId`，三区同步高亮。

## 编号规则

新增 [`frontend/src/utils/questionParts.ts`](frontend/src/utils/questionParts.ts) 辅助函数 `leafPathLabel(parts, id)`：用已有 `partPath` 把祖先 label 拼成 `(1)`、`(2)(i)`（label 本身已含括号则直接拼接）。答案/解析**只遍历叶子**，分支不出现。

## 左栏：[`EditFormSolution.vue`](frontend/src/views/edit/components/EditFormSolution.vue)

拆成三个区块，仍 `v-model` 同一棵 `parts`：

- **题干区**：现有缩进行 + label 编辑 + 子问/同级/删除；每行只留 `part.stem`（简单树不渲染本区，总前提在 QuestionEdit 的题干框）。
- **答案区**：`walkLeaves`，每叶一个 textarea 绑 `part.answer`。
- **解析区**：每叶的 `analyses` 列表、添加解法、无需解析勾选（从原来的 `leaf-body` 挪过来）。

`form.stem` 仍在 [`QuestionEdit.vue`](frontend/src/views/QuestionEdit.vue) 题干框，不搬进树组件。

## 预览渲染：[`QuestionStructureView.vue`](frontend/src/components/QuestionStructureView.vue)

由「每问一块（stem+答案+解析）」改为分组：

1. 题干：缩进渲染全部节点的 `label + stem`（简单树且无局部 stem 则跳过）
2. 答案：仅叶子，标题「答案」，行首 `leafPathLabel`
3. 解析：仅叶子，标题「解析」，多解法仍用现有分段按钮

保留 `showAnswers` / `showAnalyses`：AI 卡片已是 `false/false`，继续只显示小问题干。

[`LivePreviewCard.vue`](frontend/src/views/edit/components/LivePreviewCard.vue)：总前提仍在 `paper-stem`；其下一块 `QuestionStructureView` 输出小问题干 + 答案块 + 解析块。解答题不再走选择题那套「答案 —」的 `v-else`。

详情/列表/试卷目前把整棵树塞进「参考答案」卡片，会把小问题干和解析混在答案里。改为：

- 题干区（主 stem 下方）渲染 `showAnswers=false, showAnalyses=false`
- 「参考答案」只渲染答案段
- 解答题重新打开「解析」卡片，只渲染解析段

涉及 [`QuestionDetail.vue`](frontend/src/views/QuestionDetail.vue)、[`QuestionList.vue`](frontend/src/views/QuestionList.vue)、[`PaperDetail.vue`](frontend/src/views/PaperDetail.vue)。可给 `QuestionStructureView` 增加 `section: 'stems' | 'answers' | 'analyses' | 'all'`，避免复制三套 DOM。

## 不做

- 不改 `structure` JSON、导入、hash、AI prompt
- 不把整棵树压成一个大文本框
- 不做叶子知识点/分值
