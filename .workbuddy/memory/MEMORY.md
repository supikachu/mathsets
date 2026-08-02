# mathset 项目长期备忘

## 知识树相关架构约定（用户明确要求，不可违背）
- 后端零改动：知识树节点只走扁平 `knowledge_node_ids` 数组，落库 `question_knowledge_nodes` 关联表；**禁止**把分类信息写入 `metadata` JSONB（组卷系统需要高效 JOIN）。
- 前端分类：`loadQuestion` 时用 `knowledgeTreeApi.list()` 构建 `tree_id -> kind` 映射（chapter→章节 / ability→方法 / knowledge→知识点）分发到三个数组。
- 大树性能不用虚拟滚动，用扁平索引 Map（`Map<nodeId, {parentId, childrenIds}>`）。
- 学段/学科切换不弹窗，用内存缓存 Map（key = `${subject}_${stage}`）无缝恢复勾选。

## 前端结构要点
- `QuestionEdit.vue` 同时承担新建（isNew）与编辑；右侧属性面板 `views/edit/components/AttributeSidePanel.vue`；树组件 `components/KnowledgeTreeCheckbox.vue`（递归、受控，对外契约 `nodes` + `v-model`）。
- **知识树 UI 设计方向（用户明确拍板）**：组卷网标准文件树——每级 18px 缩进 + #dcdfe6 肘形虚线引导（li::before 竖线/li::after 横线/last-child 截断）；节点名自然换行（white-space: normal, line-height 1.5），行 align-items: flex-start，箭头/14px 复选框 margin-top 锚定首行；无搜索框/工具条/过滤按钮；容器自然撑开、滚动归最外层面板，杜绝框内嵌套滚动条；折叠动画用 grid-template-rows 0fr/1fr，禁用 max-height 上限。
- 章节/知识点/方法三组 ID 前端分开维护（chapterNodeIds / knowledgeNodeIds / methodNodeIds），提交时合并去重为 `knowledge_node_ids`。
- 图标用 `components/ui/AppIcon.vue` 内置名（有 alert、search、chevron-* 等），新增图标前先查已有。

## 重构计划
- 完整计划文件：`~/.workbuddy/cosmic-cascade-turing.md`（Phase 1-3 已全部完成于 2026-08-01）。
- 树数据共享缓存在 `frontend/src/composables/useKnowledgeTreeCache.ts`（列表全量一次 + 单树按 treeId + meta 索引），后续涉及知识树的新页面优先复用。
