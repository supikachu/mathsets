# 📕 AI 批量录题与标签管理 V2 需求分析与开发规格

> 文档版本：V2.0  
> 文档状态：可开发  
> 适用范围：数学题库系统 AI 批量录题、试卷元数据、题号关联、章节/知识点/解题方法标签管理  
> 基础文档：《AI录题时标签管理需求分析》  
> 核心方案：混合标签模式（核心知识树 + AI 自由扩展 + 智能匹配 + 人工审核）

---

# 一、项目目标

## 1.1 背景

系统支持用户上传 PDF / 图片试卷，由 AI 批量解析题目，并将题目录入题库。

现有方案已经确定三个标签维度：

1. 章节（chapter）
2. 知识点（knowledge）
3. 解题方法（method）

同时需要解决：

- 同义/近似标签重复创建
- AI 标签无法准确映射到已有标签
- 标签数量持续膨胀
- 标签层级不清晰
- 同一道题在不同试卷中题号不同
- 试卷级元数据无法结构化管理
- AI 任务失败、重试、重复落库等一致性问题

## 1.2 V2 目标

V2 必须实现：

1. AI 批量录题流程可完整落库
2. 题号只属于 `paper_question` 关系
3. 试卷元数据结构化保存
4. 三类标签支持层级、别名、规范化和生命周期管理
5. AI 标签匹配支持精确、别名、模糊/语义候选和置信度
6. 低置信度标签进入审核队列，不阻塞正常录题
7. 标签合并可追踪、可审计
8. AI Worker 支持幂等、重试和部分成功
9. 支持后续按试卷元数据 + 标签组合检索
10. 支持题目跨试卷复用和基础去重

---

# 二、范围与非目标

## 2.1 本期范围

- AI 试卷解析
- Paper 创建及元数据
- Question 创建
- PaperQuestion 关联
- Question No 管理
- Chapter / Knowledge / Method 标签管理
- 标签自动匹配
- 标签候选审核
- 标签合并
- 标签别名
- 标签层级
- AI Task 状态机
- 幂等与重试
- 基础题目去重
- 检索索引

## 2.2 本期暂不实现

以下内容不作为 V2 P0/P1 的强制交付：

- 完整向量数据库
- 自动生成完整课程知识树
- 自动重写题目内容
- 自动修改人工已经确认的标签
- 复杂知识图谱
- 多学科统一知识体系

如后续需要，可在 V3 扩展。

---

# 三、核心业务模型

## 3.1 试卷

Paper 是一次试卷录入的业务实体。

试卷包含：

- 年份
- 学段
- 学科
- 年级
- 学期
- 地区
- 学校
- 来源
- 子来源
- 教材/课标版本
- 试卷名称

## 3.2 题目

Question 是题库中的独立题目实体。

Question 不保存试卷题号。

原因：

同一道题可以被多张试卷引用：

- 试卷 A：第 1 题
- 试卷 B：第 5 题
- 试卷 C：第 17(2) 题

## 3.3 PaperQuestion

PaperQuestion 表示：

> 某题目在某张试卷中的一次引用关系。

保存：

- paper_id
- question_id
- question_no
- display_order
- score
- section_no
- sub_question_index（如有）

---

# 四、试卷元数据

## 4.1 正式字段

`papers` 建议包含：

| 字段 | 类型 | 必填 | 说明 |
|---|---|---|---|
| id | UUID | 是 | 主键 |
| name | TEXT | 是 | 试卷名称 |
| year | INT | 否 | 年份 |
| stage | TEXT | 否 | junior / senior |
| grade | TEXT | 否 | 高一/高二/高三等 |
| subject | TEXT | 否 | math / physics 等 |
| semester | TEXT | 否 | 上/下/全年 |
| curriculum_version | TEXT | 否 | 课标/教材版本 |
| region_province | TEXT | 否 | 省 |
| region_city | TEXT | 否 | 市 |
| school_id | UUID | 否 | 学校实体 |
| source_type | TEXT | 否 | 高考真题/模拟/作业等 |
| sub_source_type | TEXT | 否 | 一模/二模/联考等 |
| created_at | TIMESTAMP | 是 | 创建时间 |
| updated_at | TIMESTAMP | 是 | 更新时间 |

`ai_parse_tasks.paper_meta` 仅作为 AI 解析中间态，任务成功后同步/映射至 `papers` 正式字段。

---

# 五、AI 录题任务状态机

## 5.1 Task 状态

```text
pending
   ↓
processing
   ↓
success
```

异常情况下：

```text
processing
   ├── retrying → processing
   ├── partial_success
   └── failed
```

用户主动终止：

```text
pending / processing → cancelled
```

## 5.2 状态说明

| 状态 | 含义 |
|---|---|
| pending | 等待执行 |
| processing | AI 正在解析 |
| retrying | 自动重试 |
| success | 全部成功 |
| partial_success | 部分题目成功 |
| failed | 任务失败 |
| cancelled | 用户取消 |

## 5.3 部分成功

例如：

```text
第 1 题 ✅
第 2 题 ✅
第 3 题 ❌
第 4 题 ✅
```

Task 必须允许：

```text
partial_success
```

不能因为单题失败而回滚全部题目。

---

# 六、AI 输出 JSON Schema

建议 AI 输出统一结构：

```json
{
  "paper": {
    "name": "2025年杭州市高三数学二模",
    "year": 2025,
    "stage": "senior",
    "grade": "高三",
    "subject": "math",
    "semester": "下",
    "region_province": "浙江省",
    "region_city": "杭州市",
    "source_type": "高考模拟",
    "sub_source_type": "二模"
  },
  "questions": [
    {
      "question_no": "17(2)",
      "display_order": 17,
      "score": 8,
      "stem": "...",
      "options": [],
      "answer": "...",
      "analysis": "...",
      "question_type": "解答题",
      "difficulty": "medium",
      "chapter_path": [
        "高中数学",
        "函数",
        "导数"
      ],
      "knowledge_points": [
        {
          "name": "导数的应用",
          "confidence": 0.96
        }
      ],
      "solution_methods": [
        {
          "name": "导数法",
          "confidence": 0.91
        }
      ]
    }
  ]
}
```

## 6.1 AI 输出原则

- AI 可以输出字符串标签
- AI 不直接生成数据库 UUID
- UUID 必须由 Worker 根据标签解析结果生成
- AI 必须提供 confidence
- AI 不得直接决定两个标签是否合并
- 标签归并由系统规则 + 审核流程决定

---

# 七、标签体系

## 7.1 三个标签维度

统一使用：

```text
kind = chapter
kind = knowledge
kind = method
```

## 7.2 标签数据结构

`knowledge_nodes` 建议：

| 字段 | 类型 | 说明 |
|---|---|---|
| id | UUID | 主键 |
| kind | TEXT | chapter / knowledge / method |
| name | TEXT | 规范名称 |
| parent_id | UUID | 父节点 |
| level | INT | 层级 |
| aliases | TEXT[] | 别名 |
| canonical_id | UUID | 规范标签 |
| status | TEXT | 标签状态 |
| source | TEXT | system / admin / ai |
| usage_count | INT | 使用次数 |
| created_at | TIMESTAMP | 创建时间 |
| updated_at | TIMESTAMP | 更新时间 |

---

# 八、标签状态机

建议状态：

```text
pending_review
      ↓
active
      ↓
deprecated
```

合并时：

```text
active
  ↓
merged
  ↓
canonical_id → 规范标签
```

被管理员明确拒绝：

```text
pending_review → rejected
```

## 8.1 状态定义

| 状态 | 说明 |
|---|---|
| pending_review | AI/用户产生，尚未确认 |
| active | 正常使用 |
| merged | 已合并至其他标签 |
| deprecated | 废弃，不建议继续使用 |
| rejected | 候选标签被拒绝 |

---

# 九、标签规范化

任何 AI 标签进入数据库前，必须先经过：

```text
normalize_tag_name()
```

处理：

- 前后空格
- 多余空格
- 全角/半角
- 大小写
- 中英文标点
- 末尾句号
- 常见写法统一

示例：

```text
" 二次函数的图像。 "
        ↓
"二次函数的图像"
```

规范化结果用于匹配，但不覆盖原始 AI 输出。

建议记录：

```text
raw_name
normalized_name
```

---

# 十、标签匹配流程

## 10.1 V2 匹配流水线

```text
AI 标签
   ↓
normalize
   ↓
精确名称匹配
   ↓
alias 匹配
   ↓
fuzzy / semantic 候选召回
   ↓
confidence / 规则判断
   ↓
┌───────────────┬────────────────┬────────────────┐
│ 高置信度       │ 中置信度        │ 低置信度        │
│ 自动使用已有   │ 待审核/候选      │ 创建候选标签    │
└───────────────┴────────────────┴────────────────┘
```

## 10.2 禁止单纯使用 trigram 自动合并

`pg_trgm` 只能作为候选召回机制。

禁止：

```text
similarity > 0.85
→ 直接认为同义
→ 自动合并
```

因为：

```text
二次函数
二次函数图像
```

可能字符串高度相似，但语义不是同一个标签。

---

# 十一、标签候选机制

AI 第一次产生未知标签时：

```text
create candidate
status = pending_review
source = ai
```

而不是立即成为正式规范标签。

候选标签记录：

```text
tag_candidates
----------------
id
kind
raw_name
normalized_name
suggested_node_id
confidence
source_task_id
source_question_id
status
reviewed_by
reviewed_at
created_at
```

## 11.1 管理员操作

管理员可以：

1. 接受为新标签
2. 合并到已有标签
3. 添加为已有标签 alias
4. 拒绝
5. 修改名称后创建

---

# 十二、标签合并

## 12.1 合并规则

管理员确认：

```text
A → B
```

系统：

1. A.status = merged
2. A.canonical_id = B.id
3. A.name 加入 B.aliases
4. A 关联题目迁移/逻辑归一
5. 写入 merge history

## 12.2 合并历史

新增：

```text
tag_merge_records
```

字段：

| 字段 | 说明 |
|---|---|
| id | 主键 |
| source_tag_id | 被合并标签 |
| target_tag_id | 目标规范标签 |
| operator_type | ai / admin / system |
| reason | 合并原因 |
| confidence | 置信度 |
| created_at | 时间 |

禁止直接删除被合并标签。

---

# 十三、标签树

章节、知识点、方法均允许层级结构。

例如：

```text
高中数学
└── 函数
    └── 二次函数
        └── 二次函数图像
```

数据库：

```text
parent_id
level
```

## 13.1 层级检索

查询父节点：

```text
函数
```

默认可以根据产品筛选条件决定是否包含：

```text
一次函数
二次函数
指数函数
对数函数
```

因此 API 必须支持：

```text
include_descendants=true
```

---

# 十四、题目与试卷关系

## 14.1 `paper_question`

建议字段：

```text
paper_id
question_id
question_no
display_order
score
section_no
parent_question_id
```

## 14.2 规则

题号只保存在：

```text
paper_question.question_no
```

不得在 `questions` 表保存全局 question_no。

## 14.3 题目展示

QuestionList：

- 默认不显示题号

QuestionDetail：

- 在引用试卷中显示题号

PaperDetail：

- 按 display_order 排序
- 显示 question_no

---

# 十五、题目去重

AI 录题需要支持基础去重。

`questions` 增加：

```text
content_hash
normalized_content_hash
```

流程：

```text
AI 解析题目
 ↓
normalize question content
 ↓
计算 hash
 ↓
查询已有题目
 ↓
┌───────────────┬───────────────┐
│ 已存在         │ 不存在         │
│ 复用 question  │ 创建 question │
└───────────────┴───────────────┘
 ↓
paper_question 建立引用
```

这样同一道题出现在多张试卷中时，不重复创建 Question。

---

# 十六、幂等性

AI Worker 必须支持重复执行而不产生重复数据。

至少保证：

```text
(task_id, question_index)
```

唯一。

并根据业务需要建立：

```text
UNIQUE(paper_id, question_id)
```

Worker 重试时：

```text
先查询
 ↓
存在 → 更新/复用
不存在 → 创建
```

禁止简单：

```text
retry → INSERT
```

---

# 十七、事务边界

建议：

### Paper 创建

任务开始时创建 Paper。

### 单题处理

每道题作为独立处理单元：

```text
Question
+
PaperQuestion
+
Tag Relations
```

成功后提交。

这样单题失败不会影响其他题。

### Task 最终状态

Worker 根据：

```text
success_count
failed_count
total_count
```

计算：

```text
success
partial_success
failed
```

---

# 十八、检索需求

题目检索至少支持：

## 18.1 试卷属性

- year
- stage
- grade
- subject
- semester
- region
- school
- source_type
- sub_source_type

## 18.2 标签

- chapter
- knowledge
- method

## 18.3 题目属性

- question_type
- difficulty

## 18.4 组合逻辑

支持：

```text
AND
OR
IN
```

例如：

```text
知识点 = 二次函数
AND
难度 = 困难
```

或：

```text
知识点 IN (
  二次函数,
  指数函数
)
```

---

# 十九、索引建议

```sql
CREATE INDEX idx_paper_question_question_no
ON paper_question(paper_id, question_no);

CREATE INDEX idx_paper_question_question
ON paper_question(question_id);

CREATE INDEX idx_question_knowledge_node
ON question_knowledge_points(node_id);

CREATE INDEX idx_question_chapter_node
ON question_chapter_points(node_id);

CREATE INDEX idx_question_method_node
ON question_method_points(node_id);

CREATE INDEX idx_knowledge_nodes_canonical
ON knowledge_nodes(canonical_id)
WHERE canonical_id IS NOT NULL;

CREATE INDEX idx_knowledge_nodes_kind_name_trgm
ON knowledge_nodes
USING GIST (kind, name gist_trgm_ops);

CREATE INDEX idx_knowledge_nodes_aliases
ON knowledge_nodes
USING GIN (aliases);
```

具体索引应在实际查询计划验证后调整。

---

# 二十、前端需求

## 20.1 AI 录题前：试卷属性面板

用户上传文件后：

```text
选择文件
 ↓
试卷属性预填
 ↓
用户修改
 ↓
开始 AI 解析
```

AI 可以预填：

- 试卷名称
- 年份
- 学段
- 年级
- 学科
- 学期
- 地区
- 来源
- 子来源

## 20.2 AI 解析结果

每题展示：

```text
题号
题干
题型
难度
章节
知识点
解题方法
标签匹配状态
```

标签状态可显示：

```text
✓ 已匹配
⚠ AI 新标签
⚠ 待审核
```

---

# 二十一、后台标签管理

V2 后台至少包含：

## 21.1 标签列表

支持：

- 按维度筛选
- 名称搜索
- 状态筛选
- 来源筛选
- 使用次数排序

## 21.2 标签详情

显示：

- 标签名称
- 所属维度
- 父节点
- aliases
- 使用题目数
- 来源
- 创建时间
- canonical 标签

## 21.3 合并

操作：

```text
标签 A
↓
选择目标标签 B
↓
显示影响题目数量
↓
确认
↓
写入 merge history
```

## 21.4 审核队列

显示：

```text
AI 新标签
推荐已有标签
confidence
来源题目
来源试卷
```

管理员：

```text
接受
合并
添加 alias
拒绝
```

---

# 二十二、API 建议

## 22.1 创建 AI Task

```http
POST /ai/parse-task
```

请求：

```json
{
  "file_id": "...",
  "paper_meta": {}
}
```

## 22.2 查询 Task

```http
GET /ai/parse-task/:id
```

返回：

```json
{
  "status": "partial_success",
  "total": 20,
  "success": 18,
  "failed": 2
}
```

## 22.3 标签审核

```http
GET /admin/tag-candidates
POST /admin/tag-candidates/:id/approve
POST /admin/tag-candidates/:id/reject
POST /admin/tag-candidates/:id/merge
```

## 22.4 标签管理

```http
GET /tags
POST /tags
PATCH /tags/:id
POST /tags/:id/merge
GET /tags/:id/usage
```

---

# 二十三、异常场景

开发必须覆盖：

| 场景 | 处理 |
|---|---|
| PDF 无法解析 | task failed |
| AI 超时 | 自动重试 |
| AI 返回非法 JSON | 重试/失败 |
| 某题解析失败 | partial_success |
| 标签为空 | 记录告警，不阻塞题目落库 |
| 标签匹配失败 | candidate |
| Paper 创建成功但题目失败 | 保留 Paper + task 状态 |
| Worker 重复执行 | 幂等 |
| 标签合并 | 不删除源标签 |
| canonical 标签再次被合并 | 重新归一到最终 canonical |
| 旧题目无标签 | 允许正常使用 |

---

# 二十四、数据一致性规则

必须保证：

1. `paper_question.paper_id` 不存在时禁止插入
2. `paper_question.question_id` 不存在时禁止插入
3. merged tag 必须存在 canonical_id
4. canonical_id 不允许指向自身
5. canonical 链不能形成环
6. rejected tag 不得作为新题目的正式标签
7. deprecated tag 不建议 AI 新增
8. 旧题目标签为空时系统不能报错
9. 重试不能产生重复 Question
10. 重试不能产生重复 PaperQuestion

---

# 二十五、数据质量任务

建议每日/每周执行：

## 25.1 标签质量

检查：

- 同义标签
- 孤儿标签
- canonical 环
- 无效 parent_id
- 高增长标签
- aliases 重复

## 25.2 题目质量

检查：

- 无试卷引用题目
- 无内容题目
- 重复题目
- 无标签题目
- 标签维度缺失

## 25.3 试卷质量

检查：

- 无题试卷
- PaperQuestion 孤儿记录
- 重复题号
- display_order 冲突

---

# 二十六、非功能需求

## 26.1 检索性能

目标：

```text
章节 + 知识点 + 方法组合查询
P95 < 200ms
```

具体指标以实际数据规模和 EXPLAIN ANALYZE 验证为准。

## 26.2 数据一致性

目标：

```text
PaperQuestion 无孤儿记录
canonical 无环
Worker 重试不产生重复题目
```

## 26.3 向后兼容

现有：

```text
标签为空
paper_question 无 question_no
旧题目无新增字段
```

均不得导致已有页面或 API 报错。

---

# 二十七、实施排期

## P0：数据模型与基础链路

优先级：最高

### 数据库

- [ ] `paper_question.question_no`
- [ ] `paper_question.display_order`
- [ ] `paper_question.score`
- [ ] `ai_parse_tasks.paper_meta`
- [ ] `papers` 正式元数据字段
- [ ] `knowledge_nodes.parent_id`
- [ ] `knowledge_nodes.status`
- [ ] `knowledge_nodes.source`
- [ ] `knowledge_nodes.canonical_id`
- [ ] `knowledge_nodes.aliases`
- [ ] `tag_candidates`
- [ ] `tag_merge_records`
- [ ] question hash
- [ ] 必要索引

### Worker

- [ ] Task 状态机
- [ ] Paper 创建
- [ ] Question 创建
- [ ] PaperQuestion 关联
- [ ] 幂等
- [ ] 重试
- [ ] partial_success

---

# 二十八、P1：标签智能匹配

- [ ] normalize_tag_name
- [ ] exact match
- [ ] alias match
- [ ] pg_trgm candidate recall
- [ ] confidence
- [ ] candidate 创建
- [ ] 标签审核
- [ ] canonical 归一
- [ ] aliases 自动沉淀

---

# 二十九、P2：后台治理

- [ ] 标签树
- [ ] 标签搜索
- [ ] 标签合并
- [ ] 标签重命名
- [ ] 标签移动
- [ ] 审核队列
- [ ] merge history
- [ ] 标签使用统计
- [ ] 数据质量检测

---

# 三十、P3：智能增强

可选：

- [ ] embedding 语义召回
- [ ] AI 标签同义判断
- [ ] 自动标签质量评分
- [ ] 自动题目去重增强
- [ ] 标签推荐
- [ ] 标签体系版本化
- [ ] 多教材知识体系映射

---

# 三十一、验收标准

## 31.1 AI 录题

上传一张包含 20 道题的试卷：

- Paper 创建成功
- 20 道 Question 正确解析
- 每题存在 PaperQuestion
- 每题题号正确
- PaperQuestion display_order 正确
- 试卷元数据正确保存

## 31.2 标签

AI 输出：

```text
二次函数的图像
```

数据库已经存在：

```text
二次函数图像
```

系统不得直接创建重复正式标签。

应：

```text
匹配已有标签
或
进入候选审核
```

## 31.3 AI 新标签

AI 输出全新标签：

```text
参数分离法
```

系统：

```text
创建 candidate
status = pending_review
```

不阻塞题目落库。

## 31.4 标签合并

管理员：

```text
A → B
```

必须：

- A 标记 merged
- A.canonical_id = B
- A 不物理删除
- 题目检索仍然能够找到 B 下的相关题目
- 写入 merge history

## 31.5 Worker 重试

同一个 task 重试 3 次：

```text
Question 数量不增加
PaperQuestion 数量不增加
Tag Relation 不重复
```

## 31.6 部分失败

20 道题中 2 道失败：

```text
task.status = partial_success
success = 18
failed = 2
```

其他 18 道题正常可用。

---

# 三十二、V2 最终架构原则

本版本最终采用：

```text
                AI
                 ↓
          原始标签字符串
                 ↓
             Normalize
                 ↓
       ┌─────────┴─────────┐
       ↓                   ↓
   Exact / Alias      Candidate Recall
       ↓                   ↓
       └─────────┬─────────┘
                 ↓
           Confidence
                 ↓
       ┌─────────┼─────────┐
       ↓         ↓         ↓
     自动命中   待审核    新 Candidate
       ↓         ↓         ↓
       └─────────┴─────────┘
                 ↓
             Active Tag
                 ↓
          Canonical / Tree
                 ↓
              Question
```

核心原则：

1. **AI 不直接决定数据库 UUID**
2. **AI 不直接执行标签合并**
3. **字符串相似度只做候选召回，不直接等于语义相同**
4. **新标签先进入 Candidate**
5. **标签合并不物理删除**
6. **Question 与 Paper 解耦**
7. **Question No 属于 PaperQuestion**
8. **AI Task 必须支持幂等和部分成功**
9. **paper_meta 是中间态，papers 是最终结构化数据**
10. **人工修正结果必须沉淀为 alias / canonical 数据，形成闭环**

---

# 三十三、开发任务拆分建议

建议开发团队按以下任务拆分：

### BE-01 数据库 Migration
### BE-02 Paper Metadata API
### BE-03 AI Task 状态机
### BE-04 Worker 幂等与重试
### BE-05 Question / PaperQuestion 落库
### BE-06 Tag Normalize
### BE-07 Tag Resolver
### BE-08 Tag Candidate
### BE-09 Tag Merge
### BE-10 Tag Tree API
### BE-11 Question Dedup
### BE-12 Search Filter

### FE-01 AI 试卷元数据面板
### FE-02 AI 解析进度
### FE-03 AI 解析结果标签展示
### FE-04 标签审核页面
### FE-05 标签树管理
### FE-06 标签合并页面
### FE-07 题库高级筛选

### QA-01 AI Task 测试
### QA-02 幂等测试
### QA-03 标签匹配测试
### QA-04 标签合并测试
### QA-05 题目去重测试
### QA-06 检索性能测试

---

# 三十四、V2 与原需求的主要变化

| 项目 | V1 | V2 |
|---|---|---|
| 标签匹配 | 精确/别名/模糊 | Normalize + Exact + Alias + Candidate |
| 模糊匹配 | 可直接自动归并 | 仅候选召回 |
| 新标签 | 直接创建 | Candidate / Pending Review |
| 标签状态 | active/merged/deprecated | 增加 pending_review/rejected |
| 标签树 | 规划 | parent_id 正式落地 |
| 标签合并 | canonical_id | canonical + merge history |
| AI 置信度 | 无 | 增加 confidence |
| Paper Meta | JSONB | JSONB 中间态 + papers 正式字段 |
| Task | 基础 | 完整状态机 |
| Worker | 无明确幂等 | 必须幂等 |
| 失败处理 | 未明确 | partial_success |
| 题目去重 | 无 | hash |
| 审核 | 未明确 | Candidate Review |
| 数据治理 | 未明确 | 定期质量检查 |

---

# 三十五、最终开发结论

V2 推荐继续采用原需求中的**方案 C：混合标签模式**，但将其从：

> 核心知识树 + 自由标签 + 相似度自动合并

升级为：

> **核心知识树 + AI Candidate + 标签规范化 + 多级匹配 + 置信度 + 人工审核 + Canonical 合并 + Merge History**

同时将 AI 录题链路正式定义为：

```text
文件
 ↓
AI Parse Task
 ↓
Paper
 ↓
Questions
 ↓
PaperQuestion
 ↓
Tag Resolver
 ↓
Tag Candidate / Active Tag
 ↓
Canonical
 ↓
检索
```

该版本作为开发基线后，开发人员应以本 V2 文档为准拆分数据库 Migration、后端 API、Worker、前端页面和测试任务。
