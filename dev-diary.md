# 开发日志

## 2026-07-14 选择题多选支持 + 选项卡片重构 + 预览净化

### 一、选择题单选/多选切换功能 (QuestionEdit.vue)

新增单选/多选切换能力，数据模型通过 `sub_type` 字段区分：

- 新增 `isMultiChoice` / `multiCorrectAnswers` / `isOptionCorrect` / `hasCorrectAnswer` / `displayCorrectAnswer` 等 computed
- 新增 `switchChoiceMode()` 函数：单选→多选时将 `correctAnswer` 包装为数组，多选→单选时取第一个元素
- 编辑器选项区：多选模式下 radio 自动切换为 checkbox
- 保存逻辑：payload 增加 `sub_type` 字段，`correct_answer` 始终以数组提交
- 加载逻辑：根据 `sub_type === 'multi'` 或答案数量 > 1 自动恢复多选模式
- 草稿恢复：`sub_type` 纳入草稿存储字段列表
- `question_type` watcher 切换题型时重置 `sub_type`

### 二、顶栏布局一致性修复 (QuestionEdit.vue)

解决选择题顶栏因单选/多选按钮导致与填空题布局不一致的问题：

- 从"题型"下拉框旁移除单选/多选块状按钮
- 清理 `.meta-field-type` 的 `inline-block` / `vertical-align` 补丁样式
- 所有 9 个顶栏字段高度统一为 57.5px，完美单行对齐
- 将切换器下迁至"答案"模块标题右侧，改为精简分段控制器 `.seg-toggle`（字号 11px，浅灰底容器 + 蓝色选中态）

### 三、选项输入行 Apple 风格胶囊卡片重构 (QuestionEdit.vue)

将碎片化的选项行重构为一体化胶囊卡片：

- 结构：`.opt-row` + `.radio-label` + `.opt-input` + `.icon-btn` → 统一 `.opt-card` 容器
- 默认态：`border-radius: 10px`，`background: #f5f5f7`，`border: 1.5px solid transparent`，`padding: 8px 12px`
- 输入框：`border: none; background: transparent; box-shadow: none; outline: none`，完全融入背景
- 删除按钮：默认 `opacity: 0`，hover 整个卡片时 `opacity: 0.6` 淡入，hover 按钮自身时 `opacity: 1` + 红色高亮
- 聚焦态：`:focus-within` 触发 `border-color: var(--accent)` + `box-shadow: 0 0 0 3px var(--accent-light)`
- 选中态：`.correct` 类触发 `background: var(--accent-light)` + `border-color: var(--accent)`
- 保留 `.opt-input` CSS 供填空题使用

### 四、Chrome 预览抖动修复 (QuestionEdit.vue)

修复预览区切换正确答案时的布局抖动：

- `.paper-opt.correct` 移除 `font-weight: 600`，所有选项统一保持 `font-weight: 400`
- 切换正确答案时不再发生字体粗细变化导致的布局位移

### 五、题目详情页预览样式净化 (QuestionDetail.vue)

统一详情页选项样式为纯净试卷排版：

- `.paper-opt` 移除卡片属性（`background`、`border`、`border-radius`、`padding`、`hover`、`transition`），改为纯文本 `padding: 4px 0`
- `.paper-opt-letter` 移除圆形徽章（`width/height/border-radius/background`），改为普通加粗文字
- `.paper-opt.correct` 从"绿底+绿边框"简化为仅文字变色 `color: var(--success)`
- 新增 `isMultiChoice` computed 支持多选题识别
- 字母格式从 `A` 改为 `A.`（补上点号，与编辑器预览一致）

### 六、预览区选项前缀净化 (QuestionEdit.vue + QuestionDetail.vue)

移除预览区选项前错误出现的 ○/□ 前缀符号：

- 移除 `<span class="paper-opt-prefix">{{ isMultiChoice ? '□' : '○' }}</span>`
- 清理两个文件中残留的 `.paper-opt-prefix` 和 `.paper-opt.correct .paper-opt-prefix` CSS 规则
- 选项前缀现在纯净显示为 `A.` `B.` `C.` `D.`，复刻真实纸质试卷排版

### 七、其他修复（已有改动一并提交）

- `api/client.ts`：401 拦截器改用 `window.location.href` 跳转，避免 router/store 循环依赖导致 HMR 问题
- `stores/auth.ts`：login/logout 跳转同样改用 `window.location.href`，消除循环依赖
- `components/LatexRender.vue`：新增 `\emptyset` → `\varnothing` 宏映射 + Unicode ∅ (U+2205) 预处理，符合国内教材椭圆空集符号
