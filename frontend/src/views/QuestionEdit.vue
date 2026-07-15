<template>
  <div class="edit-page">
    <!-- 加载提示 -->
    <template v-if="!isNew && loading">
      <div class="loading-hint">加载中…</div>
    </template>

    <template v-else>
      <!-- ==================== 顶部操作栏 ==================== -->
      <header class="top-bar">
        <div class="top-bar-left">
          <AppButton variant="ghost" size="sm" @click="handleBack"><AppIcon name="chevron-left" :size="17" /> 返回</AppButton>
          <AppButton variant="ghost" size="sm" @click="handleAi"><AppIcon name="sparkles" :size="17" /> AI 智能识别</AppButton>
          <h1 class="edit-title">{{ isNew ? '录入新题' : '编辑题目' }}</h1>
          <AppBadge v-if="!isNew" color="gray">v{{ form.version }}</AppBadge>
        </div>
        <div class="top-bar-right">
          <AppButton v-if="!isNew" variant="ghost" size="sm" @click="showHistory = true"><AppIcon name="history" :size="17" /> 历史版本</AppButton>
          <AppButton variant="outline" size="sm" :loading="saving" :disabled="saving || submitting" @click="handleSave(false)"><AppIcon name="save" :size="17" /> 保存</AppButton>
          <AppButton variant="success" size="sm" :loading="submitting" :disabled="saving || submitting" @click="handleSave(true)"><AppIcon name="send" :size="17" /> 提交审核</AppButton>
        </div>
      </header>

      <!-- ==================== 第一层：核心控制元数据栏（单行不换行） ==================== -->
      <div class="meta-bar">
        <AppSelect v-model="form.question_type" :options="typeOptions" placeholder="题型" class="meta-field" />
        <div class="meta-field meta-field-diff">
          <div class="diff-row">
            <button
              v-for="n in 5"
              :key="n"
              type="button"
              class="star"
              :class="{ active: difficultyStars >= n }"
              @click="difficultyStars = n"
            ><AppIcon name="star" :size="15" /></button>
          </div>
        </div>
        <AppSelect v-model="form.academic_year" :options="academicYearOptions" placeholder="学年" clearable class="meta-field" />
        <AppSelect v-model="form.grade_semester" :options="gradeSemesterOptions" placeholder="年级学期" clearable class="meta-field" />
        <AppSelect v-model="form.exam_type" :options="examTypeOptions" placeholder="考试类型" clearable class="meta-field" />
        <input
          v-model="form.exam_region"
          placeholder="考试地区"
          class="meta-field meta-input"
        />
      </div>

      <!-- ==================== 主内容 双栏 ==================== -->
      <div class="main-content">
        <!-- 左栏：编辑 -->
        <div class="edit-col">
          <div class="edit-col-inner">
            <!-- ==================== 第二层：描述性标签流 ==================== -->
            <div class="question-tags-wrapper">
              <span v-if="form.exam_region" class="attr-tag">
                <AppIcon name="pin" :size="11" />
                <span class="attr-tag-text">{{ form.exam_region }}</span>
                <button type="button" class="attr-tag-x" @click="form.exam_region = ''"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-if="selectedKpName" class="attr-tag attr-tag-kp">
                <AppIcon name="tag" :size="11" />
                <span class="attr-tag-text">{{ selectedKpName }}</span>
                <button type="button" class="attr-tag-x" @click="clearKp"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-for="t in selectedCompetenceTags" :key="'comp-' + t.id" class="attr-tag attr-tag-literacy">
                <AppIcon name="award" :size="11" />
                <span class="attr-tag-text">{{ t.name }}</span>
                <button type="button" class="attr-tag-x" @click="toggleTagById(t)"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-for="t in selectedMethodTags" :key="'method-' + t.id" class="attr-tag attr-tag-method">
                <AppIcon name="bookmark" :size="11" />
                <span class="attr-tag-text">{{ t.name }}</span>
                <button type="button" class="attr-tag-x" @click="toggleTagById(t)"><AppIcon name="x" :size="10" /></button>
              </span>
              <span v-for="t in selectedSchoolTags" :key="'school-' + t.id" class="attr-tag attr-tag-method">
                <AppIcon name="bookmark" :size="11" />
                <span class="attr-tag-text">{{ t.name }}</span>
                <button type="button" class="attr-tag-x" @click="toggleTagById(t)"><AppIcon name="x" :size="10" /></button>
              </span>
              <button type="button" class="attr-add-btn" @click="showAttrDialog = true">
                <AppIcon name="plus" :size="13" />
                <span>添加属性</span>
              </button>
            </div>

            <!-- 题干 -->
            <section class="edit-section" :class="{ 'ai-highlight': aiGeneratedFields.has('stem') }">
              <div class="section-label"><AppIcon name="book-open" :size="16" /> <span>题干</span><span class="required">*</span></div>
              <div class="stem-wrap">
                <textarea ref="stemTextareaRef" v-model="form.stem" rows="4" class="edit-textarea stem-textarea" placeholder="输入题目内容，LaTeX 公式用 $...$ 包裹。例如：已知集合 $A = \{x | x^2 - 2x = 0\}$..." @input="autoResize"></textarea>
                <button type="button" class="img-upload-btn" @click="handleImageUpload">
                  <AppIcon name="paperclip" :size="13" />
                  <span>上传配图</span>
                </button>
              </div>
            </section>

            <!-- 答案 -->
            <section class="edit-section" :class="{ 'ai-highlight': aiGeneratedFields.has('options') || aiGeneratedFields.has('blanks') || aiGeneratedFields.has('sub_answers') }">
              <div class="section-label">
                <AppIcon name="file-text" :size="16" /> <span>答案</span>
                <div v-if="form.question_type === 'choice'" class="seg-toggle">
                  <button type="button" class="seg-btn" :class="{ active: form.sub_type !== 'multi' }" @click="switchChoiceMode('single')">单选</button>
                  <button type="button" class="seg-btn" :class="{ active: form.sub_type === 'multi' }" @click="switchChoiceMode('multi')">多选</button>
                </div>
              </div>
              <!-- 选择题选项 -->
              <div v-if="form.question_type === 'choice'" class="choice-grid">
                <div
                  v-for="(opt, i) in form.options"
                  :key="i"
                  class="opt-card"
                  :class="{ correct: isOptionCorrect(opt.label) }"
                >
                  <label class="opt-prefix" :class="{ checked: isOptionCorrect(opt.label) }">
                    <input v-if="isMultiChoice" type="checkbox" :value="opt.label" v-model="multiCorrectAnswers" />
                    <input v-else type="radio" :value="opt.label" v-model="form.correctAnswer" />
                    <span class="opt-letter">{{ opt.label }}</span>
                  </label>
                  <input
                    v-model="opt.content"
                    :placeholder="`选项 ${opt.label}`"
                    class="opt-card-input"
                    @paste="onOptionPaste($event, i)"
                  />
                  <button type="button" class="opt-img-btn" @click="handleOptionImageUpload(i)" title="上传配图">
                    <AppIcon name="paperclip" :size="14" />
                  </button>
                  <button v-if="form.options.length > 2" type="button" class="opt-delete" @click="form.options.splice(i, 1)"><AppIcon name="x" :size="15" /></button>
                </div>
                <button type="button" class="add-btn add-btn-sm" @click="addOption"><AppIcon name="plus" :size="14" /> 添加选项</button>
              </div>
              <!-- 填空题 -->
              <div v-else-if="form.question_type === 'fill'" class="blank-wrap">
                <div v-for="(blank, i) in form.blanks" :key="i" class="blank-item">
                  <span class="blank-label">第{{ i+1 }}空</span>
                  <input v-model="blank.answer" placeholder="答案" class="opt-input blank-input" />
                  <button v-if="form.blanks.length > 1" type="button" class="icon-btn" @click="form.blanks.splice(i, 1)"><AppIcon name="x" :size="15" /></button>
                </div>
                <button type="button" class="add-btn add-btn-sm" @click="form.blanks.push({ position: Math.max(...form.blanks.map(b => b.position), 0) + 1, answer: '' })"><AppIcon name="plus" :size="14" /> 添加填空位</button>
              </div>
              <!-- 解答题 -->
              <div v-else-if="form.question_type === 'solution'">
                <div class="sub-answer-list">
                  <div v-for="(ans, i) in form.sub_answers" :key="i" class="sub-answer-card">
                    <span class="sub-answer-num">({{ i + 1 }})</span>
                    <textarea
                      v-model="form.sub_answers[i]"
                      rows="2"
                      class="edit-textarea sub-answer-input"
                      :placeholder="`小题(${i + 1})答案，支持 $...$ LaTeX`"
                      @input="autoResize"
                    ></textarea>
                    <button v-if="form.sub_answers.length > 1" type="button" class="sub-answer-del" @click="removeSubAnswer(i)" title="删除此小题">
                      <AppIcon name="x" :size="14" />
                    </button>
                  </div>
                </div>
                <button type="button" class="add-btn add-btn-sm" @click="addSubAnswer">
                  <AppIcon name="plus" :size="14" /> 增加小题答案
                </button>
              </div>
            </section>

            <!-- 解析（多解法） -->
            <section class="edit-section" :class="{ 'ai-highlight': aiGeneratedFields.has('solutions') }">
              <div class="section-label"><AppIcon name="lightbulb" :size="16" /> <span>解析</span></div>
              <div class="solutions-list">
                <div v-for="(sol, i) in form.solutions" :key="i" class="solution-item">
                  <div class="solution-head">
                    <span class="solution-name">解法{{ cnNum(i + 1) }}</span>
                    <button v-if="form.solutions.length > 1" class="solution-del" @click="removeSolution(i)" title="删除此解法">
                      <AppIcon name="trash-2" :size="14" />
                    </button>
                  </div>
                  <div class="solution-textarea-wrap">
                    <textarea
                      v-model="form.solutions[i]"
                      rows="6"
                      class="edit-textarea solution-textarea"
                      :placeholder="`解法${cnNum(i + 1)}的解题思路，支持 $...$ LaTeX`"
                      @input="autoResize"
                    ></textarea>
                    <button type="button" class="img-upload-btn" @click="handleSolutionImageUpload(i)">
                      <AppIcon name="paperclip" :size="13" />
                      <span>上传配图</span>
                    </button>
                  </div>
                </div>
              </div>
              <button class="add-solution-btn" @click="addSolution">
                <AppIcon name="plus" :size="15" /> 添加新解法
              </button>
            </section>

            <!-- 高级设置（默认折叠） -->
            <section class="advanced-section">
              <button class="advanced-header" @click="toggleCollapse('collab')">
                <span class="advanced-title"><AppIcon name="users" :size="16" /> 高级设置 · 协作</span>
                <span class="collapse-arrow" :class="{ open: !collapse.collab }"><AppIcon name="chevron-down" :size="16" /></span>
              </button>
              <div v-show="!collapse.collab" class="advanced-body">
                <div class="form-grid-2">
                  <div>
                    <label class="field-label">指定审题人</label>
                    <template v-if="isTeamSpace">
                      <div v-if="spaceMembers.length === 0" class="text-sm text-muted">暂无其他团队成员</div>
                      <div v-else class="reviewer-checkboxes">
                        <label v-for="m in spaceMembers.filter(m => m.user_id !== auth.userId)" :key="m.user_id" class="reviewer-item">
                          <input type="checkbox" :value="m.user_id" v-model="form.reviewer_ids" />
                          <span>{{ m.display_name }} ({{ m.username }})</span>
                        </label>
                      </div>
                      <div class="text-sm text-muted hint-line">不选则由团队其他成员审题</div>
                    </template>
                    <div v-else class="text-sm text-muted">个人空间默认自审，无需指定</div>
                  </div>
                  <div>
                    <label class="field-label">内部备注（仅审核员可见）</label>
                    <input v-model="form.internal_note" placeholder="记录命题意图或讨论要点…" class="text-input" />
                  </div>
                </div>
              </div>
            </section>
          </div>
        </div>

        <!-- 右栏：试卷化预览 -->
        <div class="preview-col">
          <div class="preview-col-inner">
            <!-- 骨架屏（无输入时） -->
            <div v-if="!form.stem && !form.solutionAnswer && !form.solutions.some(s => s.trim()) && form.options.every(o => !o.content)" class="preview-skeleton">
              <div class="skeleton-line skeleton-title"></div>
              <div class="skeleton-line skeleton-text"></div>
              <div class="skeleton-line skeleton-text skeleton-short"></div>
              <div class="skeleton-line skeleton-text"></div>
              <div class="skeleton-gap"></div>
              <div class="skeleton-line skeleton-opt"></div>
              <div class="skeleton-line skeleton-opt"></div>
              <div class="skeleton-line skeleton-opt"></div>
              <div class="skeleton-line skeleton-opt"></div>
              <div class="skeleton-gap"></div>
              <div class="skeleton-line skeleton-answer"></div>
              <div class="skeleton-line skeleton-text skeleton-short"></div>
            </div>

            <!-- 试卷卡片（有输入时） -->
            <div v-else class="paper-card">
              <div class="paper-card-header">
                <span class="paper-type-badge">{{ typeOptions.find(t => t.value === form.question_type)?.label }}</span>
                <span class="paper-difficulty">
                  <AppIcon v-for="n in 5" :key="n" name="star" :size="12" :class="{ active: difficultyStars >= n }" class="paper-star" />
                </span>
              </div>

              <!-- 题干 -->
              <div class="paper-stem">
                <LatexRender :text="form.stem || ''" />
              </div>

              <!-- 选择题选项 -->
              <div v-if="form.question_type === 'choice' && previewOptions.length" class="paper-options" :class="'paper-options-' + optionsLayout">
                <div
                  v-for="opt in previewOptions"
                  :key="opt.label"
                  class="paper-opt"
                  :class="{ correct: isOptionCorrect(opt.label) }"
                >
                  <span class="paper-opt-letter">{{ opt.label }}.</span>
                  <LatexRender :text="opt.content" :inline="true" />
                </div>
              </div>

              <!-- 答案 & 解析 -->
              <div class="paper-answer-block">
                <div class="paper-answer-label">答案</div>
                <div class="paper-answer-content">
                  <template v-if="form.question_type === 'choice' && hasCorrectAnswer">
                    <span class="paper-correct-answer">{{ displayCorrectAnswer }}</span>
                  </template>
                  <template v-else-if="form.question_type === 'fill' && form.blanks.some(b => b.answer)">
                    <span v-for="(blank, i) in form.blanks.filter(b => b.answer)" :key="i">
                      {{ form.blanks.indexOf(blank) + 1 }}. <LatexRender :text="blank.answer" :inline="true" />&nbsp;
                    </span>
                  </template>
                  <template v-else-if="form.question_type === 'solution' && form.sub_answers.some(a => a.trim())">
                    <div v-for="(ans, i) in form.sub_answers" :key="i" class="paper-sub-answer">
                      <span class="paper-sub-num">({{ i + 1 }})</span>
                      <LatexRender :text="ans" :inline="false" />
                    </div>
                  </template>
                  <span v-else class="paper-muted">—</span>
                </div>
              </div>

              <div v-if="previewSolutions.length" class="paper-answer-block">
                <div class="paper-answer-label">
                  解析
                  <div v-if="previewSolutions.length > 1" class="sol-seg">
                    <button
                      v-for="(s, i) in previewSolutions"
                      :key="i"
                      class="sol-seg-btn"
                      :class="{ active: activeSolution === i }"
                      @click="activeSolution = i"
                    >解法{{ cnNum(i + 1) }}</button>
                  </div>
                </div>
                <div class="paper-answer-content">
                  <Transition name="sol-fade" mode="out-in">
                    <LatexRender :key="activeSolution" :text="splitSolution(previewSolutions[activeSolution]).body" />
                  </Transition>
                </div>
                <div v-if="splitSolution(previewSolutions[activeSolution]).conclusion" class="paper-conclusion">
                  <LatexRender :text="splitSolution(previewSolutions[activeSolution]).conclusion" />
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>
    </template>

    <!-- 版本历史弹窗 -->
    <AppModal v-model="showHistory" title="历史版本">
      <div class="loading-hint">版本历史功能即将上线</div>
    </AppModal>

    <!-- 属性面板 — 左右双栏 -->
    <AppModal v-model="showAttrDialog" title="属性面板">
      <div class="attr-panel">
        <!-- 左侧分类导航 -->
        <nav class="attr-panel-nav">
          <button
            class="attr-nav-item"
            :class="{ active: attrPanelTab === 'competence' }"
            @click="attrPanelTab = 'competence'"
          >
            <AppIcon name="award" :size="15" />
            <span>核心素养</span>
            <span v-if="selectedCompetenceTags.length" class="attr-nav-badge">{{ selectedCompetenceTags.length }}</span>
          </button>
          <button
            class="attr-nav-item"
            :class="{ active: attrPanelTab === 'method' }"
            @click="attrPanelTab = 'method'"
          >
            <AppIcon name="bookmark" :size="15" />
            <span>解题方法</span>
            <span v-if="selectedMethodTags.length" class="attr-nav-badge">{{ selectedMethodTags.length }}</span>
          </button>
          <button
            class="attr-nav-item"
            :class="{ active: attrPanelTab === 'school' }"
            @click="attrPanelTab = 'school'"
          >
            <AppIcon name="pin" :size="15" />
            <span>学校来源</span>
            <span v-if="selectedSchoolTags.length" class="attr-nav-badge">{{ selectedSchoolTags.length }}</span>
          </button>
        </nav>

        <!-- 右侧内容画布 -->
        <div class="attr-panel-content">
          <!-- 核心素养面板 -->
          <div v-show="attrPanelTab === 'competence'" class="attr-canvas">
            <div class="attr-canvas-hint">最多选择 {{ TAG_LIMITS.core_competence }} 个</div>
            <div class="tag-chips tag-chips-grid">
              <button
                v-for="t in competenceTags"
                :key="t.id"
                type="button"
                class="tag-chip"
                :class="{ active: form.tagIds.includes(t.id) }"
                @click="toggleTagById(t)"
              >{{ t.name }}</button>
            </div>
          </div>

          <!-- 解题方法面板 -->
          <div v-show="attrPanelTab === 'method'" class="attr-canvas">
            <div class="attr-canvas-hint">最多选择 {{ TAG_LIMITS.method }} 个</div>
            <div class="tag-chips tag-chips-grid">
              <button
                v-for="t in methodTags"
                :key="t.id"
                type="button"
                class="tag-chip"
                :class="{ active: form.tagIds.includes(t.id) }"
                @click="toggleTagById(t)"
              >{{ t.name }}</button>
            </div>
            <!-- typeahead 联想输入 -->
            <div class="typeahead-wrap">
              <input
                v-model="suggestMethod.query"
                class="attr-dialog-input"
                placeholder="搜索或创建新方法标签…"
                @input="onSuggestInput(suggestMethod, 'method')"
              />
              <div v-if="suggestMethod.results.length" class="typeahead-dropdown">
                <button
                  v-for="t in suggestMethod.results"
                  :key="t.id"
                  type="button"
                  class="typeahead-item"
                  @click="toggleTagById(t); suggestMethod.query = ''; suggestMethod.results = []"
                >
                  <span>{{ t.name }}</span>
                  <span class="typeahead-count">{{ t.use_count }} 次</span>
                </button>
              </div>
              <button
                v-if="suggestMethod.query.trim() && !suggestMethod.results.some(t => t.name === suggestMethod.query.trim())"
                type="button"
                class="typeahead-create"
                @click="createNewTag(suggestMethod.query.trim(), 'method', suggestMethod)"
              >+ 创建新标签「{{ suggestMethod.query.trim() }}」</button>
            </div>
          </div>

          <!-- 学校来源面板 -->
          <div v-show="attrPanelTab === 'school'" class="attr-canvas">
            <div class="attr-canvas-hint">最多选择 {{ TAG_LIMITS.school }} 个</div>
            <div v-if="schoolTags.length" class="tag-chips tag-chips-grid">
              <button
                v-for="t in schoolTags"
                :key="t.id"
                type="button"
                class="tag-chip"
                :class="{ active: form.tagIds.includes(t.id) }"
                @click="toggleTagById(t)"
              >{{ t.name }}</button>
            </div>
            <!-- 学校 typeahead -->
            <div class="typeahead-wrap">
              <input
                v-model="suggestSchool.query"
                class="attr-dialog-input"
                placeholder="搜索或创建学校标签…"
                @input="onSuggestInput(suggestSchool, 'school')"
              />
              <div v-if="suggestSchool.results.length" class="typeahead-dropdown">
                <button
                  v-for="t in suggestSchool.results"
                  :key="t.id"
                  type="button"
                  class="typeahead-item"
                  @click="toggleTagById(t); suggestSchool.query = ''; suggestSchool.results = []"
                >
                  <span>{{ t.name }}</span>
                  <span class="typeahead-count">{{ t.use_count }} 次</span>
                </button>
              </div>
              <button
                v-if="suggestSchool.query.trim() && !suggestSchool.results.some(t => t.name === suggestSchool.query.trim())"
                type="button"
                class="typeahead-create"
                @click="createNewTag(suggestSchool.query.trim(), 'school', suggestSchool)"
              >+ 创建新标签「{{ suggestSchool.query.trim() }}」</button>
            </div>
          </div>
        </div>
      </div>
      <div class="form-actions">
        <AppButton variant="primary" @click="showAttrDialog = false">完成</AppButton>
      </div>
    </AppModal>

    <!-- 离开确认 -->
    <AppConfirm
      v-model="leaveDialog"
      title="未保存提示"
      message="有未保存的修改，确定离开吗？"
      confirm-text="离开"
      danger
      @confirm="goBack"
    />

    <!-- 草稿恢复确认 -->
    <AppConfirm
      v-model="restoreDialog"
      title="恢复草稿"
      message="检测到未保存的草稿，是否恢复？"
      confirm-text="恢复"
      cancel-text="丢弃"
      @confirm="doRestoreDraft"
      @update:model-value="(v: boolean) => { if (!v) discardDraft() }"
    />

    <!-- AI 智能识别弹窗 -->
    <AppModal v-model="showAiDialog" title="AI 智能识别" size="lg">
      <div class="ai-dialog-body">
        <!-- 输入区 -->
        <div v-if="!aiResult" class="ai-input-section">
          <!-- 模式切换 Tab -->
          <div class="ai-mode-tabs">
            <button :class="{ active: aiMode === 'api' }" @click="aiMode = 'api'">API 智能解析</button>
            <button :class="{ active: aiMode === 'markdown' }" @click="aiMode = 'markdown'">Markdown 粘贴</button>
          </div>

          <!-- Markdown 模式：提示词复制区 -->
          <div v-if="aiMode === 'markdown'" class="ai-prompt-section">
            <div class="ai-prompt-header">
              <span class="ai-prompt-title">第一步：复制提示词</span>
              <AppButton variant="outline" size="sm" @click="copyPrompt">
                <AppIcon name="copy" :size="14" /> {{ promptCopied ? '已复制' : '复制提示词' }}
              </AppButton>
            </div>
            <div class="ai-prompt-preview">{{ RECOMMENDED_PROMPT }}</div>
            <div class="ai-steps">
              ① 复制提示词 → ② 打开 AI 网页上传题目图片并粘贴提示词 → ③ 复制 AI 输出 → ④ 粘贴到下方
            </div>
          </div>

          <p v-if="aiMode === 'api'" class="ai-hint">粘贴题目文本（含题干、选项、答案、解析），AI 将自动识别结构并填入表单。</p>
          <p v-else class="ai-hint">粘贴 AI 按推荐格式输出的 Markdown，系统将自动解析并填入表单。</p>
          <textarea
            v-model="aiText"
            class="ai-textarea"
            rows="10"
            :placeholder="aiMode === 'markdown'
              ? '在此粘贴 AI 输出的 Markdown...'
              : '例如：\n已知函数 f(x) = 2x + 1，求 f(3) 的值。\n解：f(3) = 2×3 + 1 = 7'"
          ></textarea>
          <div v-if="aiError" class="ai-error">{{ aiError }}</div>
          <div class="ai-actions">
            <AppButton variant="ghost" @click="showAiDialog = false">取消</AppButton>
            <AppButton variant="primary" :loading="aiParsing" @click="doAiParse">
              <AppIcon name="sparkles" :size="16" /> {{ aiParsing ? '解析中…' : '开始识别' }}
            </AppButton>
          </div>
        </div>

        <!-- 结果预览 -->
        <div v-else class="ai-result-section">
          <div class="ai-result-meta">
            <AppBadge :color="aiResult.confidence >= 0.8 ? 'green' : aiResult.confidence >= 0.5 ? 'yellow' : 'red'">
              置信度 {{ Math.round(aiResult.confidence * 100) }}%
            </AppBadge>
            <span class="ai-result-type">{{ ({ choice: '选择题', fill: '填空题', solution: '解答题' } as Record<string, string>)[aiResult.question_type] }}</span>
          </div>
          <div v-if="aiResult.warnings.length" class="ai-warnings">
            <div v-for="(w, i) in aiResult.warnings" :key="i" class="ai-warning-item">⚠ {{ w }}</div>
          </div>
          <div class="ai-result-preview">
            <div class="ai-preview-block">
              <div class="ai-preview-label">题干</div>
              <div class="ai-preview-content">{{ aiResult.stem }}</div>
            </div>
            <div v-if="aiResult.options?.length" class="ai-preview-block">
              <div class="ai-preview-label">选项</div>
              <div v-for="opt in aiResult.options" :key="opt.label" class="ai-preview-option">
                <span class="ai-opt-label">{{ opt.label }}.</span> {{ opt.content }}
              </div>
            </div>
            <div class="ai-preview-block">
              <div class="ai-preview-label">答案</div>
              <div class="ai-preview-content">
                <span v-if="aiResult.correct_answer.kind === 'choice'">{{ aiResult.correct_answer.value.options?.join(', ') }}</span>
                <span v-else-if="aiResult.correct_answer.kind === 'fill'">{{ aiResult.correct_answer.value.blanks?.map(b => b.answer).join('、') }}</span>
                <span v-else>{{ aiResult.correct_answer.value.subs?.map(s => s.content).join('；') }}</span>
              </div>
            </div>
            <div class="ai-preview-block">
              <div class="ai-preview-label">解析（{{ aiResult.analysis.length }} 种解法）</div>
              <div v-for="(a, i) in aiResult.analysis" :key="i" class="ai-preview-analysis">
                <strong>{{ a.title }}</strong>
                <div>{{ a.content }}</div>
              </div>
            </div>
          </div>
          <div class="ai-actions">
            <AppButton variant="ghost" @click="aiResult = null">返回修改</AppButton>
            <AppButton variant="success" @click="applyAiResult"><AppIcon name="check" :size="16" /> 应用到表单</AppButton>
          </div>
        </div>
      </div>
    </AppModal>

    <!-- AI 覆盖二次确认 -->
    <AppConfirm
      v-model="aiDirtyConfirm"
      title="AI 覆盖确认"
      message="AI 解析将覆盖当前已填写的内容，是否继续？"
      confirm-text="覆盖"
      danger
      @confirm="confirmAiOverwrite"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, reactive, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { useRoute, useRouter } from 'vue-router'
import { questionApi, kpApi, spaceApi, aiApi, tagsApi, type KnowledgePoint, type SpaceMemberInfo, type ParsedQuestion, type TagSummary, type Tag } from '@/api/client'
import LatexRender from '@/components/LatexRender.vue'
import { AppButton, AppBadge, AppModal, AppConfirm, AppEmpty, AppSelect, AppIcon } from '@/components/ui'
import { useToast } from '@/composables/useToast'
import { useSpaceStore } from '@/stores/space'
import { useAuthStore } from '@/stores/auth'
import { useSelectedKp } from '@/composables/useSelectedKp'
import { parseMarkdownToQuestion, RECOMMENDED_PROMPT } from '@/utils/parseMarkdown'

const route = useRoute()
const router = useRouter()
const toast = useToast()
const space = useSpaceStore()
const auth = useAuthStore()
const { selectedKpId, selectedKpName, select: selectKp, clear: clearKp } = useSelectedKp()
const isNew = route.path.endsWith('/new')
const loading = ref(false)
const saving = ref(false)
const submitting = ref(false)
const isLoading = ref(false)
const kpLoading = ref(false)
const kpTree = ref<KnowledgePoint[]>([])
const showHistory = ref(false)
const showAttrDialog = ref(false)
const attrPanelTab = ref<'competence' | 'method' | 'school'>('competence')
const grades = ['初一', '初二', '初三', '高一', '高二', '高三']

// 标签分类数据：从后端 API 动态加载
const methodTags = ref<Tag[]>([])        // 解题方法（含数学思想、技巧等，category=method）
const competenceTags = ref<Tag[]>([])     // 核心素养（category=core_competence）
const schoolTags = ref<Tag[]>([])         // 学校（category=school）

// 标签数量上限（防呆软限制）
const TAG_LIMITS: Record<string, number> = {
  core_competence: 3,
  method: 5,
  knowledge_point: 3,
  school: 1,
}

// 标签加载状态
const tagsLoading = ref(false)
async function loadTags() {
  tagsLoading.value = true
  try {
    const [methodRes, compRes, schoolRes] = await Promise.all([
      tagsApi.list('method'),
      tagsApi.list('core_competence'),
      tagsApi.list('school'),
    ])
    methodTags.value = methodRes.data
    competenceTags.value = compRes.data
    schoolTags.value = schoolRes.data
  } catch { /* handled */ }
  finally { tagsLoading.value = false }
}

// 统一标签切换（基于 tag ID，含数量防呆）
function toggleTagById(tag: TagSummary) {
  const idx = form.tagIds.indexOf(tag.id)
  if (idx >= 0) {
    form.tagIds.splice(idx, 1)
    return
  }
  // 防呆：检查该 category 数量上限
  const count = form_tagList.value.filter(t => t.category === tag.category).length
  const limit = TAG_LIMITS[tag.category] ?? 99
  if (count >= limit) {
    const labels: Record<string, string> = { core_competence: '核心素养', method: '解题方法', school: '学校' }
    toast.warning(`${labels[tag.category] || '标签'}最多选择 ${limit} 个`)
    return
  }
  form.tagIds.push(tag.id)
}

// 知识点数量防呆
function checkKpLimit(): boolean {
  const kpCount = selectedKpId.value ? 1 : 0
  if (kpCount >= TAG_LIMITS.knowledge_point) {
    toast.warning(`知识点最多选择 ${TAG_LIMITS.knowledge_point} 个`)
    return false
  }
  return true
}

// 标签 typeahead 联想状态（method 和 school 独立，避免串扰）
interface SuggestState {
  query: string
  results: Tag[]
  loading: boolean
  timer: ReturnType<typeof setTimeout> | null
}
const suggestMethod = reactive<SuggestState>({ query: '', results: [], loading: false, timer: null })
const suggestSchool = reactive<SuggestState>({ query: '', results: [], loading: false, timer: null })

function onSuggestInput(state: SuggestState, category: 'method' | 'school') {
  if (state.timer) clearTimeout(state.timer)
  const q = state.query.trim()
  if (!q) {
    state.results = []
    return
  }
  state.timer = setTimeout(async () => {
    state.loading = true
    try {
      const res = await tagsApi.suggest(q, category)
      state.results = res.data
    } catch { state.results = [] }
    finally { state.loading = false }
  }, 200)
}

// 创建新标签（typeahead 无匹配时）
async function createNewTag(name: string, category: 'method' | 'school', state: SuggestState) {
  try {
    const res = await tagsApi.create(name, category)
    // 刷新对应列表
    if (category === 'method') {
      methodTags.value = [...methodTags.value, res.data]
    } else {
      schoolTags.value = [...schoolTags.value, res.data]
    }
    // 自动选中
    form.tagIds.push(res.data.id)
    toast.success(`已创建并选中标签「${name}」`)
    state.query = ''
    state.results = []
  } catch (e: any) {
    toast.error(e.response?.data?.error || '创建标签失败')
  }
}

const gradeOptions = grades.map((g) => ({ label: g, value: g }))

// 子题型已移除

// 学年选项
const currentYear = new Date().getFullYear()
const academicYearOptions = [
  { label: `${currentYear - 1}-${String(currentYear).slice(2)}`, value: `${currentYear - 1}-${String(currentYear).slice(2)}` },
  { label: `${currentYear}-${String(currentYear + 1).slice(2)}`, value: `${currentYear}-${String(currentYear + 1).slice(2)}` },
  { label: `${currentYear + 1}-${String(currentYear + 2).slice(2)}`, value: `${currentYear + 1}-${String(currentYear + 2).slice(2)}` },
]

// 年级学期选项（合并年级+学期）
const gradeSemesterOptions = [
  ...['初一', '初二', '初三'].flatMap(g => [
    { label: `${g}上`, value: `${g}上` },
    { label: `${g}下`, value: `${g}下` },
  ]),
  ...['高一', '高二', '高三'].flatMap(g => [
    { label: `${g}上`, value: `${g}上` },
    { label: `${g}下`, value: `${g}下` },
  ]),
]

// 考试类型选项
const examTypeOptions = [
  { label: '期末', value: '期末' },
  { label: '期中', value: '期中' },
  { label: '月考', value: '月考' },
  { label: '周测', value: '周测' },
  { label: '模拟', value: '模拟' },
  { label: '高考', value: '高考' },
  { label: '中考', value: '中考' },
  { label: '竞赛', value: '竞赛' },
]

const sourceOptions = [
  { label: '原创', value: '原创' },
  { label: '改编', value: '改编' },
  { label: '高考真题', value: '高考真题' },
  { label: '模拟题', value: '模拟题' },
  { label: '名校试卷', value: '名校试卷' },
]
const typeOptions = [
  { label: '选择题', value: 'choice' },
  { label: '填空题', value: 'fill' },
  { label: '解答题', value: 'solution' },
]
const semesterOptions = [
  { label: '上学期', value: '上学期' },
  { label: '下学期', value: '下学期' },
  { label: '全学年', value: '全学年' },
]
const reviewerOptions = ref<{ label: string; value: string }[]>([])
const spaceMembers = ref<SpaceMemberInfo[]>([])

// 当前空间是否为团队空间（团队空间才显示审题人选择）
const isTeamSpace = computed(() => space.currentSpace?.kind === 'team')

// 预览区选择题选项（computed 缓存，避免每次渲染都 filter 产生新数组导致重渲染抖动）
const previewOptions = computed(() => {
  if (!Array.isArray(form.options)) return []
  return form.options.filter(o => o.content)
})

// ===== 单选/多选切换 =====
const isMultiChoice = computed(() => form.question_type === 'choice' && form.sub_type === 'multi')

// 多选答案数组（与 form.correctAnswer 双向同步）
const multiCorrectAnswers = computed({
  get: () => Array.isArray(form.correctAnswer) ? form.correctAnswer : [],
  set: (val: string[]) => { form.correctAnswer = [...val].sort() },
})

function isOptionCorrect(label: string): boolean {
  if (Array.isArray(form.correctAnswer)) return form.correctAnswer.includes(label)
  return form.correctAnswer === label
}

const hasCorrectAnswer = computed(() => {
  if (Array.isArray(form.correctAnswer)) return form.correctAnswer.length > 0
  return !!form.correctAnswer
})

const displayCorrectAnswer = computed(() => {
  if (Array.isArray(form.correctAnswer)) return [...form.correctAnswer].sort().join('')
  return form.correctAnswer
})

function switchChoiceMode(mode: 'single' | 'multi') {
  if (mode === 'multi') {
    form.sub_type = 'multi'
    // 单选答案转多选数组
    if (form.correctAnswer && !Array.isArray(form.correctAnswer)) {
      form.correctAnswer = [form.correctAnswer]
    } else if (!form.correctAnswer) {
      form.correctAnswer = []
    }
  } else {
    form.sub_type = ''
    // 多选数组取第一个转单选
    if (Array.isArray(form.correctAnswer)) {
      form.correctAnswer = form.correctAnswer[0] || ''
    }
  }
}

// 可折叠面板
const collapse = reactive({
  source: true,
  basic: true,
  collab: true,
})
function toggleCollapse(key: keyof typeof collapse) {
  collapse[key] = !collapse[key]
}

// 可拖拽分隔条
const splitRatio = ref(0.55)
const isDragging = ref(false)
const currentRow = ref(-1)
const rowRefs = [ref<HTMLElement>(), ref<HTMLElement>(), ref<HTMLElement>()]

function startResize(rowIdx: number, _e: MouseEvent) {
  isDragging.value = true
  currentRow.value = rowIdx
  document.body.style.cursor = 'col-resize'
  document.body.style.userSelect = 'none'
  document.addEventListener('mousemove', onMouseMove)
  document.addEventListener('mouseup', stopResize)
}

function onMouseMove(e: MouseEvent) {
  if (!isDragging.value) return
  const idx = currentRow.value
  if (idx < 0 || idx >= rowRefs.length) return
  const el = rowRefs[idx]?.value
  if (!el) return
  const rect = el.getBoundingClientRect()
  const x = e.clientX - rect.left
  let ratio = x / rect.width
  ratio = Math.max(0.2, Math.min(0.8, ratio))
  splitRatio.value = ratio
}

function stopResize() {
  isDragging.value = false
  currentRow.value = -1
  document.body.style.cursor = ''
  document.body.style.userSelect = ''
  document.removeEventListener('mousemove', onMouseMove)
  document.removeEventListener('mouseup', stopResize)
}

// 难度映射
const diffLabels = ['简单', '较易', '中等', '较难', '困难']
const _diffStars = ref(3)
const difficultyStars = computed({
  get: () => _diffStars.value,
  set: (v: number) => {
    _diffStars.value = v
    // 5星难度系数: 1→0.9, 2→0.75, 3→0.55, 4→0.35, 5→0.2
    form.difficulty_coefficient = [0.9, 0.75, 0.55, 0.35, 0.2][v - 1] ?? 0.55
    // 同步 difficulty 字段：1-2→easy, 3→medium, 4-5→hard
    form.difficulty = v <= 2 ? 'easy' : v === 3 ? 'medium' : 'hard'
  },
})

const form = reactive({
  stem: '',
  question_type: 'choice',
  sub_type: '' as string,
  difficulty: 'medium',
  difficulty_coefficient: 0.5 as number,
  default_score: 5,
  grade: undefined as string | undefined,
  semester: undefined as string | undefined,
  academic_year: '' as string,
  grade_semester: '' as string,
  exam_region: '' as string,
  exam_type: '' as string,
  source: '原创',
  estimated_time: 5,
  solutions: [''] as string[],
  options: [
    { label: 'A', content: '' },
    { label: 'B', content: '' },
    { label: 'C', content: '' },
    { label: 'D', content: '' },
  ] as { label: string; content: string }[],
  correctAnswer: '' as string | string[],
  blanks: [{ position: 1, answer: '' }] as { position: number; answer: string }[],
  solutionAnswer: '',
  sub_answers: [''] as string[],
  gradingSteps: [] as { label: string; points: number; description: string }[],
  judgmentCorrect: true,
  knowledgePointIds: [] as string[],
  tagIds: [] as string[],          // 统一标签 ID 列表（核心素养 + 解题方法 + 学校）
  reviewer: '' as string,
  reviewer_ids: [] as string[],
  internal_note: '',
  status: '',
  version: 1,
  hasUnsaved: false,
})

// 标签查找缓存：将 tagIds 映射为 TagSummary 对象（用于标签流显示和 category 分流）
const allTagsMap = computed(() => {
  const m = new Map<string, TagSummary>()
  for (const t of methodTags.value) m.set(t.id, t)
  for (const t of competenceTags.value) m.set(t.id, t)
  for (const t of schoolTags.value) m.set(t.id, t)
  return m
})

// 当前选中的标签列表（TagSummary 对象数组）
const form_tagList = computed<TagSummary[]>(() => {
  return form.tagIds
    .map(id => allTagsMap.value.get(id))
    .filter((t): t is TagSummary => !!t)
})

// 按 category 分流（便于标签流 UI 显示）
const selectedCompetenceTags = computed(() => form_tagList.value.filter(t => t.category === 'core_competence'))
const selectedMethodTags = computed(() => form_tagList.value.filter(t => t.category === 'method'))
const selectedSchoolTags = computed(() => form_tagList.value.filter(t => t.category === 'school'))

// ===== 返回检测 =====
const leaveDialog = ref(false)
function handleBack() {
  if (form.hasUnsaved) {
    leaveDialog.value = true
  } else {
    goBack()
  }
}
function goBack() {
  // 优先用 router.back() 回退，不产生重复历史条目
  if (window.history.state?.back) {
    router.back()
  } else {
    if (isNew) router.replace('/questions')
    else router.replace(`/questions/${route.params.id}`)
  }
}

// ===== AI 智能识别 =====
const showAiDialog = ref(false)
const aiText = ref('')
const aiParsing = ref(false)
const aiResult = ref<ParsedQuestion | null>(null)
const aiError = ref('')
// AI 痕迹高亮：记录哪些字段被 AI 填充
const aiGeneratedFields = ref<Set<string>>(new Set())
// 解析模式：api 调用后端 / markdown 前端纯解析
const aiMode = ref<'api' | 'markdown'>('api')
const promptCopied = ref(false)
// AI 结果应用期间跳过 watch 对 question_type 的重置（避免覆盖刚赋好的 sub_answers）
const applyingAiResult = ref(false)

// AI Dirty Check：检测表单是否有手动输入内容
function isFormDirty(): boolean {
  if (form.stem.trim()) return true
  if (form.options.some(o => o.content.trim())) return true
  if (form.solutions.some(s => s.trim())) return true
  if (form.blanks.some(b => b.answer.trim())) return true
  if (form.sub_answers.some(a => a.trim())) return true
  if (form.tagIds.length > 0) return true
  return false
}

// AI 覆盖二次确认弹窗
const aiDirtyConfirm = ref(false)
// 暂存待应用的 AI 结果（确认后执行）
let pendingAiApply: ParsedQuestion | null = null

function handleAi() {
  showAiDialog.value = true
  aiError.value = ''
  aiResult.value = null
}

// 复制提示词到剪贴板
async function copyPrompt() {
  try {
    await navigator.clipboard.writeText(RECOMMENDED_PROMPT)
    toast.success('提示词已复制，请粘贴到 AI 对话框使用')
    promptCopied.value = true
    setTimeout(() => { promptCopied.value = false }, 3000)
  } catch {
    toast.error('复制失败，请手动选择提示词文本复制')
  }
}

async function doAiParse() {
  if (!aiText.value.trim()) {
    toast.warning('请输入题目文本')
    return
  }
  aiParsing.value = true
  aiError.value = ''
  aiResult.value = null
  try {
    if (aiMode.value === 'markdown') {
      // 纯前端解析，不调用 API
      aiResult.value = parseMarkdownToQuestion(aiText.value)
    } else {
      const res = await aiApi.parseText(aiText.value)
      aiResult.value = res.data.data
    }
  } catch (e: any) {
    aiError.value = e.response?.data?.error || e.message || 'AI 解析失败'
  } finally {
    aiParsing.value = false
  }
}

// §4.4a: 应用 AI 结果 — 映射前先重置题型依赖数组，防止旧题型残留污染
function applyAiResult() {
  const q = aiResult.value
  if (!q) return

  // Dirty Check：表单有手动输入内容时弹出二次确认
  if (isFormDirty()) {
    pendingAiApply = q
    aiDirtyConfirm.value = true
    return
  }

  doApplyAiResult(q)
}

// 确认覆盖后执行
function confirmAiOverwrite() {
  aiDirtyConfirm.value = false
  if (pendingAiApply) {
    doApplyAiResult(pendingAiApply)
    pendingAiApply = null
  }
}

function doApplyAiResult(q: ParsedQuestion) {

  // 标记 AI 应用中，阻止 watch(question_type) 重置 sub_answers 等
  applyingAiResult.value = true

  // 强制重置题型相关的依赖数组
  form.options = []
  form.blanks = []
  form.sub_answers = ['']
  form.correctAnswer = ''
  form.solutions = ['']
  aiGeneratedFields.value = new Set()

  // 题型
  form.question_type = q.question_type
  form.sub_type = q.sub_type || ''
  aiGeneratedFields.value.add('question_type')

  // 题干
  form.stem = q.stem
  aiGeneratedFields.value.add('stem')

  // 难度（如果 AI 返回了）
  if (q.difficulty) {
    form.difficulty = q.difficulty
    const diffMap: Record<string, number> = { easy: 2, medium: 3, hard: 4 }
    _diffStars.value = diffMap[q.difficulty] || 3
    form.difficulty_coefficient = [0.9, 0.75, 0.55, 0.35, 0.2][_diffStars.value - 1] ?? 0.55
    aiGeneratedFields.value.add('difficulty')
  }

  // 答案（按题型分支）
  if (q.question_type === 'choice' && q.options) {
    form.options = q.options.map(o => ({ label: o.label, content: o.content }))
    if (q.correct_answer.kind === 'choice' && q.correct_answer.value.options) {
      const opts = q.correct_answer.value.options
      if (q.sub_type === 'multi' || opts.length > 1) {
        form.sub_type = 'multi'
        form.correctAnswer = opts
      } else {
        form.correctAnswer = opts[0] || ''
      }
    }
    aiGeneratedFields.value.add('options')
    aiGeneratedFields.value.add('correctAnswer')
  } else if (q.question_type === 'fill') {
    if (q.correct_answer.kind === 'fill' && q.correct_answer.value.blanks) {
      form.blanks = q.correct_answer.value.blanks.map(b => ({ position: b.position, answer: b.answer }))
    }
    aiGeneratedFields.value.add('blanks')
  } else if (q.question_type === 'solution') {
    if (q.correct_answer.kind === 'solution' && q.correct_answer.value.subs) {
      form.sub_answers = q.correct_answer.value.subs.map(s => s.content)
    }
    aiGeneratedFields.value.add('sub_answers')
  }

  // 解析（多解法 → solutions 数组，保存时用 \n\n---\n\n 拼接）
  form.solutions = q.analysis.map(a => a.content)
  aiGeneratedFields.value.add('solutions')

  // 知识点匹配：使用后端返回的 kp_matches（高置信度自动选中）
  if (q.kp_matches?.length) {
    // 取第一个 score >= 0.95 的高置信度匹配自动选中
    const highConfidenceMatch = q.kp_matches.find(m => m.score >= 0.95 && m.matched_id)
    if (highConfidenceMatch) {
      selectKp(highConfidenceMatch.matched_id!, highConfidenceMatch.matched_name!)
      aiGeneratedFields.value.add('knowledge_point')
    }
  }

  form.hasUnsaved = true
  showAiDialog.value = false
  toast.success('AI 识别结果已应用')

  // 程序化赋值不会触发 @input，需在 DOM 更新后手动重算 textarea 高度；
  // 同时在 watch 回调执行后解除标志位
  nextTick(() => {
    applyingAiResult.value = false
    resizeAllTextareas()
  })

  // 痕迹高亮 8 秒后淡出
  setTimeout(() => {
    aiGeneratedFields.value.clear()
  }, 8000)
}

// ===== 选项增删 =====
function addOption() {
  const labels = 'ABCDEFGH'
  const i = form.options.length
  if (i < 8) form.options.push({ label: labels[i], content: '' })
}

// 题干配图上传
const stemTextareaRef = ref<HTMLTextAreaElement>()
function handleImageUpload() {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = 'image/png,image/jpeg,image/gif,image/webp'
  input.onchange = async () => {
    const file = input.files?.[0]
    if (!file) return
    if (file.size > 5 * 1024 * 1024) {
      toast.error('图片不能超过 5MB')
      return
    }
    // 使用 Blob URL（短链接），避免 base64 海量字符污染文本域
    const imageUrl = URL.createObjectURL(file)
    const ta = stemTextareaRef.value
    if (!ta) {
      form.stem += `\n![题干配图](${imageUrl})\n`
      return
    }
    const pos = ta.selectionStart
    const before = form.stem.substring(0, pos)
    const after = form.stem.substring(ta.selectionEnd)
    const insert = `\n![题干配图](${imageUrl})\n`
    form.stem = before + insert + after
    nextTick(() => {
      ta.focus()
      const newPos = pos + insert.length
      ta.setSelectionRange(newPos, newPos)
      resizeTextarea(ta)
    })
  }
  input.click()
}

// 选项配图上传（按索引定位对应 input）
function handleOptionImageUpload(index: number) {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = 'image/png,image/jpeg,image/gif,image/webp'
  input.onchange = async () => {
    const file = input.files?.[0]
    if (!file) return
    if (file.size > 5 * 1024 * 1024) {
      toast.error('图片不能超过 5MB')
      return
    }
    const imageUrl = URL.createObjectURL(file)
    const inp = document.querySelectorAll<HTMLInputElement>('.opt-card-input')[index]
    if (!inp) {
      form.options[index].content += `![选项配图](${imageUrl})`
      return
    }
    const pos = inp.selectionStart ?? 0
    const before = form.options[index].content.substring(0, pos)
    const after = form.options[index].content.substring(inp.selectionEnd ?? 0)
    const insert = `![选项配图](${imageUrl})`
    form.options[index].content = before + insert + after
    nextTick(() => {
      inp.focus()
      const newPos = pos + insert.length
      inp.setSelectionRange(newPos, newPos)
    })
  }
  input.click()
}

// 选项输入框粘贴图片
function onOptionPaste(e: ClipboardEvent, index: number) {
  const items = e.clipboardData?.items
  if (!items) return
  for (const item of items) {
    if (item.type.startsWith('image/')) {
      e.preventDefault()
      const file = item.getAsFile()
      if (!file) return
      if (file.size > 5 * 1024 * 1024) {
        toast.error('图片不能超过 5MB')
        return
      }
      const imageUrl = URL.createObjectURL(file)
      const inp = e.target as HTMLInputElement
      const pos = inp.selectionStart ?? 0
      const before = form.options[index].content.substring(0, pos)
      const after = form.options[index].content.substring(inp.selectionEnd ?? 0)
      const insert = `![选项配图](${imageUrl})`
      form.options[index].content = before + insert + after
      nextTick(() => {
        inp.focus()
        const newPos = pos + insert.length
        inp.setSelectionRange(newPos, newPos)
      })
      break
    }
  }
}

// 预览区选项布局智能判定（Layout Calculator）
// 返回 '1col' | '2col' | '4col'
const optionsLayout = computed(() => {
  const opts = previewOptions.value
  if (opts.length === 0) return '2col'

  // 任何选项含图片标记 → 单列
  if (opts.some(opt => opt.content.includes('!['))) return '1col'

  // 预估单选项渲染宽度（粗略：LaTeX 命令字符不计入，中文2字符宽，英文1字符宽）
  function estimateWidth(text: string): number {
    // 去除 LaTeX 命令的影响：$...$ 内的公式按字符数粗估
    let w = 0
    let inFormula = false
    for (const ch of text) {
      if (ch === '$') { inFormula = !inFormula; continue }
      if (inFormula && /\\[a-zA-Z]+/.test(text)) {
        // LaTeX 命令大致按 1.5 个字符宽估算
        w += 0.6
      } else if (/[\u4e00-\u9fff]/.test(ch)) {
        w += 2 // 中文字符约 2 个单位宽
      } else if (ch === '\\') {
        w += 0 // 跳过反斜杠
      } else {
        w += 1
      }
    }
    // 加上 "A. " 前缀约 2 个单位
    return w + 2
  }

  // 预览区可用宽度估算：容器约 380px，每字符约 8px → 约 47 单位
  // 但用比例更稳定：W/4 ≈ 12 单位，W/2 ≈ 24 单位
  const QUARTER_W = 14  // W/4 阈值（含字母前缀）
  const HALF_W = 28     // W/2 阈值

  const widths = opts.map(opt => estimateWidth(opt.content))
  const maxW = Math.max(...widths)

  if (maxW > HALF_W) return '1col'
  if (maxW > QUARTER_W) return '2col'
  return '4col'
})

// 兼容旧引用
const shouldUseSingleColumn = computed(() => optionsLayout.value === '1col')
function handleSolutionImageUpload(index: number) {
  const input = document.createElement('input')
  input.type = 'file'
  input.accept = 'image/png,image/jpeg,image/gif,image/webp'
  input.onchange = async () => {
    const file = input.files?.[0]
    if (!file) return
    if (file.size > 5 * 1024 * 1024) {
      toast.error('图片不能超过 5MB')
      return
    }
    const imageUrl = URL.createObjectURL(file)
    const ta = document.querySelectorAll<HTMLTextAreaElement>('.solution-textarea')[index]
    if (!ta) {
      form.solutions[index] += `\n![解析配图](${imageUrl})\n`
      return
    }
    const pos = ta.selectionStart
    const before = form.solutions[index].substring(0, pos)
    const after = form.solutions[index].substring(ta.selectionEnd)
    const insert = `\n![解析配图](${imageUrl})\n`
    form.solutions[index] = before + insert + after
    nextTick(() => {
      ta.focus()
      const newPos = pos + insert.length
      ta.setSelectionRange(newPos, newPos)
      resizeTextarea(ta)
    })
  }
  input.click()
}
function resizeTextarea(el: HTMLTextAreaElement) {
  el.style.height = 'auto'
  el.style.height = el.scrollHeight + 'px'
}
function autoResize(e: Event) {
  resizeTextarea(e.target as HTMLTextAreaElement)
}
// 对页面内所有 .edit-textarea 执行自适应（用于加载已有题目后手动触发）
function resizeAllTextareas() {
  document.querySelectorAll<HTMLTextAreaElement>('.edit-textarea').forEach(el => {
    resizeTextarea(el)
  })
}

// ===== 多解法 =====
const cnNums = ['一', '二', '三', '四', '五', '六', '七', '八', '九', '十']
function cnNum(n: number): string {
  return cnNums[n - 1] || String(n)
}

function addSolution() {
  form.solutions.push('')
  nextTick(() => {
    const els = document.querySelectorAll<HTMLTextAreaElement>('.solution-textarea')
    els[els.length - 1]?.focus()
  })
}

function removeSolution(i: number) {
  form.solutions.splice(i, 1)
  if (form.solutions.length === 0) form.solutions.push('')
  if (activeSolution.value >= form.solutions.length) activeSolution.value = form.solutions.length - 1
}

// ===== 多小题答案 =====
function addSubAnswer() {
  form.sub_answers.push('')
  nextTick(() => {
    const els = document.querySelectorAll<HTMLTextAreaElement>('.sub-answer-input')
    const last = els[els.length - 1]
    if (last) {
      resizeTextarea(last)
      last.focus()
    }
  })
}

function removeSubAnswer(i: number) {
  form.sub_answers.splice(i, 1)
  if (form.sub_answers.length === 0) form.sub_answers.push('')
}

const activeSolution = ref(0)

const previewSolutions = computed(() => form.solutions.filter(s => s.trim()))

// 抽离结论性文字（"故选X"、"故答案为..."等）
function splitSolution(text: string): { body: string; conclusion: string } {
  if (!text) return { body: '', conclusion: '' }
  // 匹配结尾的结论性语句
  const patterns = [
    /(?:故|因此|所以|综上)[选答]\s*[A-Z](?:[、,，]\s*[A-Z])*\s*。?\s*$/,
    /(?:故|因此|所以|综上)[^。\n]*答案[^。\n]*[。]?\s*$/,
    /(?:故|因此|所以|综上)[^。\n]*[。]?\s*$/,
    /故选\s*[A-Z](?:[、,，]\s*[A-Z])*\s*。?\s*$/,
  ]
  for (const p of patterns) {
    const m = text.match(p)
    if (m) {
      const idx = text.lastIndexOf(m[0])
      return { body: text.substring(0, idx).trim(), conclusion: m[0].trim() }
    }
  }
  return { body: text.trim(), conclusion: '' }
}

// ===== 构建提交数据 =====
function buildPayload() {
  // 知识点来自左侧知识树选中项
  const kpIds = selectedKpId.value ? [selectedKpId.value] : (form.knowledgePointIds.length > 0 ? form.knowledgePointIds : [])
  const payload: any = {
    stem: form.stem,
    question_type: form.question_type,
    sub_type: form.sub_type || null,
    difficulty: form.difficulty,
    difficulty_coefficient: form.difficulty_coefficient,
    default_score: form.default_score,
    grade: form.grade || null,
    semester: form.semester || null,
    academic_year: form.academic_year || null,
    grade_semester: form.grade_semester || null,
    exam_region: form.exam_region || null,
    exam_type: form.exam_type || null,
    source: form.source,
    analysis: form.solutions.filter(s => s.trim()).join('\n\n---\n\n') || null,
    knowledge_point_ids: kpIds.length > 0 ? kpIds : null,
    tag_ids: form.tagIds,
  }
  switch (form.question_type) {
    case 'choice':
      payload.options = (form.options || []).filter(o => o.content.trim())
      payload.sub_type = form.sub_type || null
      if (Array.isArray(form.correctAnswer)) {
        payload.correct_answer = form.correctAnswer
      } else {
        payload.correct_answer = form.correctAnswer ? [form.correctAnswer] : []
      }
      break
    case 'fill':
      payload.correct_answer = form.blanks.filter(b => b.answer.trim()).map(b => ({ position: b.position, answer: b.answer.trim() }))
      break
    case 'solution':
      payload.correct_answer = form.sub_answers.filter(a => a.trim())
      break
    case 'judgment':
      payload.correct_answer = [form.judgmentCorrect]
      break
  }
  return payload
}

// ===== 保存 =====
async function handleSave(submitAfter: boolean) {
  if (!form.stem.trim()) { toast.warning('请输入题干'); return }
  if (form.question_type === 'choice' && !hasCorrectAnswer.value) { toast.warning('请选择正确答案'); return }
  const flag = submitAfter ? submitting : saving
  flag.value = true
  try {
    const data = buildPayload()
    const res = isNew ? await questionApi.create(data) : await questionApi.update(route.params.id as string, data)
    const qid = res.data.id
    form.hasUnsaved = false
    clearDraft()
    if (submitAfter) {
      await questionApi.submit(qid, { reviewer_ids: form.reviewer_ids.length > 0 ? form.reviewer_ids : undefined })
      toast.success('已创建并提交审核')
    }
    else { toast.success(isNew ? '草稿已保存' : '已更新') }
    // 保存后跳转详情页：新题用 replace 替换编辑页；已有题用 back() 回退到来源详情页
    if (isNew) {
      router.replace(`/questions/${qid}`)
    } else {
      if (window.history.state?.back) {
        router.back()
      } else {
        router.replace(`/questions/${qid}`)
      }
    }
  } catch (e: any) { toast.error(e.response?.data?.error || '操作失败') }
  finally { flag.value = false }
}

// ===== 自动保存草稿 =====
let autoSaveTimer: ReturnType<typeof setTimeout> | null = null
watch(() => ({ ...form }), () => {
  if (isLoading.value) return
  form.hasUnsaved = true
  if (autoSaveTimer) clearTimeout(autoSaveTimer)
  autoSaveTimer = setTimeout(() => {
    try {
      const key = isNew ? 'q-draft-new' : `q-draft-${route.params.id}`
      sessionStorage.setItem(key, JSON.stringify(form))
    } catch { /* quota exceeded */ }
  }, 3000)
}, { deep: true })

// ===== 自动草稿恢复 =====
const restoreDialog = ref(false)
let pendingDraft: any = null

function getDraftKey() {
  return isNew ? 'q-draft-new' : `q-draft-${route.params.id}`
}

function restoreDraft() {
  const key = getDraftKey()
  try {
    const saved = sessionStorage.getItem(key)
    if (!saved) return
    const draft = JSON.parse(saved)
    if (draft.stem || draft.solutions?.some((s: string) => s?.trim()) || draft.solutionAnswer) {
      pendingDraft = draft
      restoreDialog.value = true
    }
  } catch { /* ignore */ }
}

function doRestoreDraft() {
  if (!pendingDraft) return
  const fields = ['stem', 'question_type', 'sub_type', 'difficulty', 'default_score', 'grade', 'semester',
    'source', 'solutions', 'options', 'correctAnswer', 'blanks', 'solutionAnswer', 'sub_answers',
    'gradingSteps', 'judgmentCorrect', 'knowledgePointIds', 'tagIds', 'difficulty_coefficient', 'academic_year', 'grade_semester', 'exam_region', 'exam_type', 'reviewer', 'reviewer_ids', 'internal_note']
  for (const f of fields) {
    if (pendingDraft[f] !== undefined) (form as any)[f] = pendingDraft[f]
  }
  toast.success('草稿已恢复')
  pendingDraft = null
  restoreDialog.value = false
}

function discardDraft() {
  try { sessionStorage.removeItem(getDraftKey()) } catch { /* ignore */ }
  pendingDraft = null
}

function clearDraft() {
  try { sessionStorage.removeItem(getDraftKey()) }
  catch { /* ignore */ }
}

async function loadKpTree() {
  kpLoading.value = true
  try {
    const res = await kpApi.tree(); kpTree.value = res.data
  } catch { /* handled */ }
  finally { kpLoading.value = false }
}

async function loadSpaceMembers() {
  if (!isTeamSpace.value || !space.currentSpaceId) return
  try {
    const res = await spaceApi.get(space.currentSpaceId)
    spaceMembers.value = res.data.members || []
  } catch { /* handled */ }
}

async function loadQuestion() {
  if (isNew) return
  isLoading.value = true
  loading.value = true
  try {
    const res = await questionApi.get(route.params.id as string)
    const d = res.data
    form.stem = d.stem
    form.question_type = d.question_type
    form.difficulty = d.difficulty
    form.default_score = d.default_score
    form.grade = d.grade || undefined
    form.semester = d.semester || undefined
    form.sub_type = (d as any).sub_type || ''
    form.difficulty_coefficient = (d as any).difficulty_coefficient ?? 0.5
    form.academic_year = d.academic_year || ''
    form.grade_semester = d.grade_semester || ''
    form.exam_region = d.exam_region || ''
    form.exam_type = d.exam_type || ''
    form.source = d.source || '原创'
    const raw = d.analysis || ''
    if (raw.includes('\n\n---\n\n')) {
      form.solutions = raw.split(/\n\n---\n\n/)
    } else if (/\n解法[二三四五六七八九十]/.test(raw)) {
      // 迁移旧数据：按"解法二"、"解法三"等文本标记自动拆分
      form.solutions = raw.split(/\n(?=解法[二三四五六七八九十])/).map(s => s.trim())
    } else {
      form.solutions = raw ? [raw] : ['']
    }
    form.status = d.status
    form.version = d.version
    form.knowledgePointIds = d.knowledge_points?.map(k => k.id) || []
    form.tagIds = d.tags?.map(t => t.id) || []
    // 将后端返回的标签（可能是空间私有标签）合并到本地缓存，确保 allTagsMap 能找到
    if (d.tags?.length) {
      for (const t of d.tags) {
        if (!allTagsMap.value.has(t.id)) {
          const fullTag: Tag = { ...t, space_id: null, use_count: 0, created_at: '' }
          if (t.category === 'core_competence') competenceTags.value = [...competenceTags.value, fullTag]
          else if (t.category === 'method') methodTags.value = [...methodTags.value, fullTag]
          else if (t.category === 'school') schoolTags.value = [...schoolTags.value, fullTag]
        }
      }
    }
    form.correctAnswer = ''
    form.blanks = [{ position: 1, answer: '' }]
    form.solutionAnswer = ''
    form.sub_answers = ['']
    form.gradingSteps = []
    form.judgmentCorrect = true
    if (d.question_type === 'choice' && d.options) {
      let opts = d.options
      if (typeof opts === 'string') { try { opts = JSON.parse(opts) } catch { opts = [] } }
      if (Array.isArray(opts)) {
        form.options = opts.map((opt: any) => {
          if (typeof opt === 'string') return { label: opt[0] || '', content: opt.slice(1).trim() }
          if (opt && typeof opt === 'object' && opt.label) return { label: opt.label, content: opt.content || '' }
          if (opt && typeof opt === 'object') return { label: Object.keys(opt)[0], content: Object.values(opt)[0] as string }
          return { label: '', content: String(opt) }
        })
      }
      if (Array.isArray(d.correct_answer)) {
        if ((d as any).sub_type === 'multi' || d.correct_answer.length > 1) {
          form.sub_type = 'multi'
          form.correctAnswer = d.correct_answer as string[]
        } else {
          form.correctAnswer = d.correct_answer[0] || ''
        }
      }
    } else if (d.question_type === 'fill' && Array.isArray(d.correct_answer)) {
      form.blanks = (d.correct_answer as any[]).map((b: any) => ({ position: b.position, answer: b.answer }))
    } else if (d.question_type === 'solution') {
      if (Array.isArray(d.correct_answer) && d.correct_answer.length > 0) {
        form.sub_answers = d.correct_answer.map((a: any) => typeof a === 'string' ? a : String(a))
      }
    } else if (d.question_type === 'judgment') {
      if (Array.isArray(d.correct_answer)) form.judgmentCorrect = d.correct_answer[0] === true
    }
    form.hasUnsaved = false
  } catch { /* handled */ }
  finally {
    loading.value = false
    // 等待 watcher 在 isLoading=true 时刷新（避免误触发 hasUnsaved）
    await nextTick()
    isLoading.value = false
    // 程序化设置的值不会触发 @input，需手动调整文本框高度
    if (!isNew) {
      await nextTick()
      resizeAllTextareas()
    }
  }
}

// ===== 窗口关闭检测 =====
function handleBeforeUnload(e: BeforeUnloadEvent) {
  if (form.hasUnsaved) { e.preventDefault(); e.returnValue = '' }
}
onMounted(() => {
  window.addEventListener('beforeunload', handleBeforeUnload)
  loadKpTree()
  loadSpaceMembers()
  loadTags()
  loadQuestion().then(() => {
    if (!isNew) restoreDraft()
  })
  if (isNew) restoreDraft()
})
onBeforeUnmount(() => {
  window.removeEventListener('beforeunload', handleBeforeUnload)
  if (autoSaveTimer) clearTimeout(autoSaveTimer)
  stopResize()
})

watch(() => form.question_type, () => {
  // AI 应用期间跳过重置，避免覆盖 applyAiResult 刚赋好的 sub_answers/blanks/options
  if (isNew && !applyingAiResult.value) {
    form.sub_type = ''
    form.correctAnswer = ''
    form.blanks = [{ position: 1, answer: '' }]
    form.solutionAnswer = ''
    form.sub_answers = ['']
    form.gradingSteps = []
    form.judgmentCorrect = true
  }
})
</script>

<style scoped>
.edit-page {
  height: 100vh;
  display: flex;
  flex-direction: column;
  overflow: hidden;
  padding: 16px 24px;
  gap: 12px;
  background: var(--bg-primary);
}

.edit-title {
  font-size: 17px;
  font-weight: 650;
  margin: 0 0 0 2px;
  color: var(--text-primary);
  letter-spacing: -0.01em;
}

.loading-hint {
  text-align: center;
  padding: 48px 20px;
  color: var(--text-muted);
  font-size: 14px;
}

/* ============ 顶部操作栏 ============ */
.top-bar {
  display: flex;
  align-items: center;
  justify-content: space-between;
  flex-shrink: 0;
  gap: 12px;
}

.top-bar-left,
.top-bar-right {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* ============ 题型分段控件 + 难度 ============ */
/* 题型字段宽度 */
.meta-field-type {
  min-width: 80px;
}

/* 难度字段 */
.meta-field-diff {
  min-width: 100px;
}

.diff-row {
  display: flex;
  align-items: center;
  gap: 1px;
  min-height: auto;
}

.star {
  color: var(--border-strong);
  background: none;
  border: none;
  cursor: pointer;
  padding: 4px;
  display: inline-flex;
  transition: var(--transition-fast);
}

/* SVG 图标不拦截点击，确保点击事件落在 button 上 */
.star :deep(svg),
.star svg {
  pointer-events: none;
}

.star:hover {
  transform: scale(1.12);
}

.star.active {
  color: var(--star-color);
}

/* 激活态星星图标覆盖降噪色 — 确保 SVG currentColor 生效 */
.star.active :deep(svg),
.star.active svg {
  color: var(--star-color) !important;
}

/* 难度系数输入 */
.diff-coef-input {
  width: 36px;
  padding: 0 4px;
  border: none;
  border-radius: 0;
  background: transparent;
  color: var(--text-primary);
  font-size: 12px;
  text-align: center;
  margin-left: 4px;
  font-family: inherit;
  box-sizing: border-box;
  min-height: auto;
  outline: none;
}

.diff-coef-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

/* ============ 元数据工具栏 — 第一层：核心控制元数据栏（单行不换行） ============ */
.meta-bar {
  display: flex;
  align-items: center;
  white-space: nowrap;
  overflow-x: auto;
  gap: 8px;
  flex-shrink: 0;
  padding: 10px 14px;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-xs);
  scrollbar-width: thin;
}

.meta-bar::-webkit-scrollbar {
  height: 4px;
}

.meta-bar::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 2px;
}

/* 通用胶囊样式 — 适用于 AppSelect、input、button、div */
.meta-field {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  flex: initial;
  min-width: 0;
  padding: 4px 16px;
  height: 32px;
  border-radius: 9999px;
  background: #fff;
  border: 1px solid #e0e0e0;
  color: #8c8c8c;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.2s ease;
  position: relative;
  box-sizing: border-box;
  white-space: nowrap;
}

/* 已选择/激活状态：文字加深、边框加深、淡雅背景 */
.meta-field :deep(.app-select-trigger.has-value),
.meta-field.text-input:not(:placeholder-shown) {
  color: #262626;
}

.meta-field.app-select-wrapper:has(.app-select-trigger.has-value) {
  color: #262626;
  border-color: #b7b7b7;
  background: rgba(0, 122, 255, 0.03);
}

[data-theme='dark'] .meta-field.app-select-wrapper:has(.app-select-trigger.has-value) {
  color: rgba(255, 255, 255, 0.95);
  border-color: rgba(255, 255, 255, 0.25);
  background: rgba(0, 122, 255, 0.08);
}

.meta-field.text-input:not(:placeholder-shown) {
  color: #262626;
  border-color: #b7b7b7;
  background: rgba(0, 122, 255, 0.03);
}

[data-theme='dark'] .meta-field.text-input:not(:placeholder-shown) {
  color: rgba(255, 255, 255, 0.95);
  border-color: rgba(255, 255, 255, 0.25);
  background: rgba(0, 122, 255, 0.08);
}

/* 暗色模式 */
[data-theme='dark'] .meta-field {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.12);
  color: rgba(255, 255, 255, 0.5);
}

.meta-field:hover {
  border-color: var(--accent);
}

.meta-field:focus-within {
  border-color: #b7b7b7;
  box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.06);
}

/* AppSelect 直接作为 meta-field 时的胶囊样式 */
.meta-field.app-select-wrapper {
  display: inline-flex;
  width: auto;
  padding: 4px 16px;
  height: 32px;
  border-radius: 9999px;
  background: #fff;
  border: 1px solid #e0e0e0;
  color: #8c8c8c;
  box-sizing: border-box;
}

/* 内层 trigger 彻底隐形化 — 由外层胶囊接管全部视觉 */
.meta-field :deep(.app-select-trigger) {
  border: none !important;
  background: transparent !important;
  outline: none !important;
  box-shadow: none !important;
  appearance: none;
  -webkit-appearance: none;
  padding: 0 !important;
  min-height: auto !important;
  height: 100%;
  width: 100%;
  font-size: 13px;
  color: inherit;
  border-radius: 0;
}

.meta-field :deep(.app-select-trigger:hover) {
  border: none !important;
  background: transparent !important;
  box-shadow: none !important;
}

.meta-field :deep(.app-select-trigger.open) {
  border: none !important;
  box-shadow: none !important;
  background: transparent !important;
}

.meta-field :deep(.app-select-text) {
  white-space: nowrap;
  color: inherit;
}

.meta-field :deep(.app-select-text.placeholder) {
  color: var(--text-muted);
}

/* text-input 作为胶囊 */
.meta-field.text-input {
  border: 1px solid #d9d9d9;
  background: #fff;
  border-radius: 9999px;
  padding: 4px 16px;
  height: 32px;
  font-size: 13px;
  color: var(--text-primary);
  outline: none;
  width: auto;
  max-width: 140px;
}

[data-theme='dark'] .meta-field.text-input {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.15);
  color: rgba(255, 255, 255, 0.9);
}

.meta-field.text-input::placeholder {
  color: var(--text-muted);
}

.meta-field.text-input:focus {
  border-color: #b7b7b7;
  box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.06);
}

/* 考试地区平铺输入框（与 AppSelect 胶囊视觉一致） */
.meta-input {
  border: 1px solid #e0e0e0;
  background: #fff;
  border-radius: 9999px;
  padding: 4px 16px;
  height: 32px;
  font-size: 13px;
  color: #8c8c8c;
  outline: none;
  width: auto;
  max-width: 120px;
  box-sizing: border-box;
  transition: all 0.2s ease;
}

.meta-input:not(:placeholder-shown) {
  color: #262626;
  border-color: #b7b7b7;
  background: rgba(0, 122, 255, 0.03);
}

.meta-input::placeholder {
  color: var(--text-muted);
}

.meta-input:hover {
  border-color: var(--accent);
}

.meta-input:focus {
  border-color: #b7b7b7;
  box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.06);
}

[data-theme='dark'] .meta-input {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.12);
  color: rgba(255, 255, 255, 0.5);
}

[data-theme='dark'] .meta-input:not(:placeholder-shown) {
  color: rgba(255, 255, 255, 0.95);
  border-color: rgba(255, 255, 255, 0.25);
  background: rgba(0, 122, 255, 0.08);
}

/* 难度星级胶囊 — 对称内边距呼吸感 */
.meta-field-diff {
  gap: 2px;
  padding: 0 16px;
}

.diff-row {
  display: flex;
  align-items: center;
  gap: 2px;
  min-height: auto;
}

/* ============ 第二层：描述性标签流区域 ============ */
.question-tags-wrapper {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin-bottom: 12px;
  padding: 10px 12px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  min-height: 44px;
}

/* 空状态时隐藏 wrapper 避免多余间距 */
.question-tags-wrapper:empty {
  display: none;
}

/* 属性标签胶囊 — 苹果风半透明浅色 */
.attr-tag {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 3px 4px 3px 10px;
  border-radius: 9999px;
  font-size: 12px;
  font-weight: 500;
  color: var(--text-secondary);
  background: rgba(0, 122, 255, 0.06);
  border: 1px solid rgba(0, 122, 255, 0.12);
  transition: all 0.2s ease;
  white-space: nowrap;
  max-width: 200px;
}

[data-theme='dark'] .attr-tag {
  background: rgba(0, 122, 255, 0.1);
  border-color: rgba(0, 122, 255, 0.18);
  color: rgba(255, 255, 255, 0.85);
}

/* 知识点标签 — 蓝色系 */
.attr-tag-kp {
  background: rgba(0, 122, 255, 0.08);
  border-color: rgba(0, 122, 255, 0.18);
  color: var(--accent);
}

/* 核心素养标签 — 紫色系 */
.attr-tag-literacy {
  background: rgba(175, 82, 222, 0.06);
  border-color: rgba(175, 82, 222, 0.15);
  color: #7b2cbf;
}

[data-theme='dark'] .attr-tag-literacy {
  background: rgba(175, 82, 222, 0.12);
  border-color: rgba(175, 82, 222, 0.2);
  color: #c77dff;
}

/* 解题方法标签 — 绿色系 */
.attr-tag-method {
  background: rgba(52, 199, 89, 0.06);
  border-color: rgba(52, 199, 89, 0.15);
  color: #1a7a37;
}

[data-theme='dark'] .attr-tag-method {
  background: rgba(52, 199, 89, 0.12);
  border-color: rgba(52, 199, 89, 0.2);
  color: #6bce8a;
}

.attr-tag :deep(svg),
.attr-tag svg {
  width: 11px !important;
  height: 11px !important;
  flex-shrink: 0;
  opacity: 0.7;
}

.attr-tag-text {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

/* 标签删除手柄 */
.attr-tag-x {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 16px;
  height: 16px;
  border: none;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.06);
  color: var(--text-muted);
  cursor: pointer;
  flex-shrink: 0;
  transition: all 0.15s ease;
  padding: 0;
}

.attr-tag-x:hover {
  background: var(--danger);
  color: #fff;
}

[data-theme='dark'] .attr-tag-x {
  background: rgba(255, 255, 255, 0.1);
}

.attr-tag-x :deep(svg),
.attr-tag-x svg {
  width: 10px !important;
  height: 10px !important;
}

/* 添加属性辅助按钮 */
.attr-add-btn {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 4px 12px;
  border-radius: 9999px;
  border: 1px dashed var(--border-strong);
  background: transparent;
  color: var(--text-muted);
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s ease;
  white-space: nowrap;
  font-family: inherit;
}

.attr-add-btn:hover {
  border-color: var(--accent);
  border-style: solid;
  color: var(--accent);
  background: var(--accent-light);
}

.attr-add-btn :deep(svg),
.attr-add-btn svg {
  width: 13px !important;
  height: 13px !important;
}

/* 降噪图标系统 — 统一 14px + 灰色 */
.meta-field :deep(svg),
.meta-field .app-icon,
.meta-field svg {
  width: 14px !important;
  height: 14px !important;
  color: #999;
  flex-shrink: 0;
}

[data-theme='dark'] .meta-field :deep(svg),
[data-theme='dark'] .meta-field .app-icon,
[data-theme='dark'] .meta-field svg {
  color: rgba(255, 255, 255, 0.4);
}

/* AppSelect 箭头图标也降噪 */
.meta-field :deep(.app-select-chevron) {
  color: #999;
}

[data-theme='dark'] .meta-field :deep(.app-select-chevron) {
  color: rgba(255, 255, 255, 0.4);
}

/* ============ 主内容双栏 ============ */
.main-content {
  flex: 1;
  overflow: hidden;
  display: flex;
  gap: 14px;
  min-height: 0;
}

.edit-col {
  flex: 0 0 55%;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-card);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  box-shadow: var(--shadow-xs);
  overflow: hidden;
}

.edit-col-inner {
  flex: 1;
  overflow-y: auto;
  padding: 14px 16px;
}

.preview-col {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  overflow: hidden;
}

.preview-col-inner {
  flex: 1;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  padding: 20px;
  box-sizing: border-box;
}

/* ============ 编辑区段 ============ */
.edit-section {
  margin-bottom: 14px;
}

.edit-section:last-child {
  margin-bottom: 0;
}

.section-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  font-weight: 650;
  text-transform: uppercase;
  letter-spacing: 0.04em;
  color: var(--text-secondary);
  margin-bottom: 10px;
}

.section-label .required {
  color: var(--danger);
  margin-left: 2px;
}

.edit-textarea {
  width: 100%;
  padding: 10px 12px;
  border-radius: var(--radius-sm);
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.6;
  font-family: var(--font-cn-isolated);
  resize: none;
  overflow-y: hidden;
  transition: var(--transition-fast);
  box-sizing: border-box;
}

.edit-textarea:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

/* 题干输入框 - 最低高度120px */
.stem-textarea {
  min-height: 120px;
}

/* 解法输入框 */
.solution-textarea {
  min-height: 120px;
}

.solution-textarea-wrap {
  position: relative;
}

.solution-textarea-wrap .solution-textarea {
  padding-bottom: 35px;
}

.solutions-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.solution-item {
  border: 1px solid var(--border-color);
  border-radius: var(--radius-md);
  overflow: hidden;
  transition: border-color 0.2s ease;
}

.solution-item:focus-within {
  border-color: var(--accent);
}

.solution-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 6px 12px;
  background: var(--bg-input);
  border-bottom: 1px solid var(--border-color);
}

.solution-name {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-secondary);
}

.solution-del {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  border-radius: var(--radius-sm);
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  transition: all 0.2s ease;
}

.solution-del:hover {
  background: var(--danger-light);
  color: var(--danger);
}

.solution-textarea {
  border: none !important;
  border-radius: 0 !important;
}

.add-solution-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  margin-top: 10px;
  padding: 8px 14px;
  border: 1px dashed var(--border-color);
  border-radius: var(--radius-md);
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s ease;
}

.add-solution-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

/* 预览端分段切换 */
.sol-seg {
  display: inline-flex;
  gap: 2px;
  padding: 2px;
  border-radius: var(--radius-full);
  background: var(--bg-input);
}

.sol-seg-btn {
  padding: 3px 10px;
  border: none;
  border-radius: var(--radius-full);
  background: transparent;
  font-size: 11px;
  font-weight: 500;
  color: var(--text-muted);
  cursor: pointer;
  transition: all 0.2s ease;
}

.sol-seg-btn.active {
  background: var(--bg-card);
  color: var(--accent);
  box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
}

[data-theme='dark'] .sol-seg-btn.active {
  background: rgba(255, 255, 255, 0.12);
}

/* 结论高亮区 */
.paper-conclusion {
  margin-top: 12px;
  padding: 10px 14px;
  border-radius: var(--radius-md);
  background: var(--accent-light);
  border-left: 3px solid var(--accent);
  font-size: 14px;
  color: var(--text-primary);
}

/* 淡入淡出过渡 */
.sol-fade-enter-active,
.sol-fade-leave-active {
  transition: opacity 0.2s ease;
}

.sol-fade-enter-from,
.sol-fade-leave-to {
  opacity: 0;
}

/* 题干容器 - 图片按钮挂载在右下角 */
.stem-wrap {
  position: relative;
}

.stem-wrap .edit-textarea {
  padding-bottom: 35px;
}

/* 图片上传按钮 - 挂载在题干右下角内部 */
.img-upload-btn {
  position: absolute;
  bottom: 5px;
  right: 8px;
  display: inline-flex;
  align-items: center;
  gap: 3px;
  padding: 1px 8px;
  border: 1px dashed var(--border-strong);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.8);
  color: var(--text-muted);
  font-size: 11px;
  font-family: inherit;
  cursor: pointer;
  transition: var(--transition-fast);
  line-height: 1.5;
  z-index: 1;
}

[data-theme='dark'] .img-upload-btn {
  background: rgba(28, 28, 30, 0.8);
}

.img-upload-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

/* 选择题选项 Grid */
.choice-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}

.choice-grid .add-btn-sm {
  grid-column: 1;
  justify-self: start;
  width: fit-content;
  max-width: 200px;
}

/* 填空题紧凑布局 */
.blank-wrap {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: flex-end;
}

.blank-item {
  display: flex;
  align-items: center;
  gap: 4px;
}

.blank-input {
  width: 100px !important;
  flex: none !important;
}

/* 选项卡片（一体化胶囊） */
.opt-card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  border-radius: 10px;
  background: #f5f5f7;
  border: 1.5px solid transparent;
  transition: border-color 0.2s ease, background-color 0.2s ease, box-shadow 0.2s ease;
}

[data-theme='dark'] .opt-card {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.08);
}

.opt-card:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

[data-theme='dark'] .opt-card:focus-within {
  box-shadow: 0 0 0 3px rgba(0, 122, 255, 0.15);
}

.opt-card.correct {
  background: var(--accent-light);
  border-color: var(--accent);
}

[data-theme='dark'] .opt-card.correct {
  background: rgba(0, 122, 255, 0.12);
  border-color: var(--accent);
}

/* 前缀（单选/多选 + 字母） */
.opt-prefix {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  flex-shrink: 0;
  cursor: pointer;
  user-select: none;
}

.opt-prefix input {
  margin: 0;
  accent-color: var(--accent);
}

.opt-letter {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-secondary);
}

.opt-prefix.checked .opt-letter {
  color: var(--accent);
}

/* 隐形输入框 */
.opt-card-input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  box-shadow: none;
  outline: none;
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.4;
  font-family: inherit;
  padding: 2px 0;
}

.opt-card-input::placeholder {
  color: var(--text-muted);
}

/* 删除按钮（hover 淡入） */
.opt-delete {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  border-radius: 6px;
  opacity: 0;
  transition: opacity 0.2s ease, color 0.2s ease, background-color 0.2s ease;
}

.opt-card:hover .opt-delete {
  opacity: 0.6;
}

.opt-delete:hover {
  opacity: 1 !important;
  color: var(--danger);
  background: var(--danger-light);
}

/* 选项配图按钮（hover/focus 淡入） */
.opt-img-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  flex-shrink: 0;
  width: 24px;
  height: 24px;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  border-radius: 6px;
  opacity: 0;
  transition: opacity 0.2s ease, color 0.2s ease, background-color 0.2s ease;
}

.opt-card:hover .opt-img-btn,
.opt-card:focus-within .opt-img-btn {
  opacity: 0.6;
}

.opt-img-btn:hover {
  opacity: 1 !important;
  color: var(--accent);
  background: var(--accent-light);
}

.step-input {
  flex: 1;
  min-width: 0;
  padding: 8px 12px;
  border-radius: 8px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.4;
  transition: var(--transition-fast);
  font-family: inherit;
}

.step-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.opt-input {
  flex: 1;
  min-width: 0;
  padding: 6px 32px 6px 10px;
  border-radius: 8px;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  color: var(--text-primary);
  font-size: 13px;
  line-height: 1.4;
  transition: var(--transition-fast);
  font-family: inherit;
}

.opt-input:focus {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
  background: var(--bg-card);
}

.blank-label {
  font-size: 12px;
  color: var(--text-muted);
  width: 44px;
  flex-shrink: 0;
  font-weight: 550;
}

.grading-label {
  display: none;
}

/* 动态小题答案卡片组 */
.sub-answer-list {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.sub-answer-card {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  position: relative;
  background: var(--bg-input);
  border: 1px solid var(--border-color);
  border-radius: 12px;
  padding: 10px 12px;
  transition: border-color 0.2s;
}

.sub-answer-card:focus-within {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

[data-theme='dark'] .sub-answer-card {
  background: rgba(255, 255, 255, 0.04);
  border-color: rgba(255, 255, 255, 0.08);
}

.sub-answer-num {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  flex-shrink: 0;
  padding-top: 6px;
  min-width: 24px;
}

.sub-answer-input {
  flex: 1;
  border: none !important;
  background: transparent !important;
  padding: 4px 0 !important;
  min-height: 32px;
  font-family: var(--font-cn-isolated);
}

.sub-answer-input:focus {
  border: none !important;
  box-shadow: none !important;
}

.sub-answer-del {
  position: absolute;
  top: 6px;
  right: 6px;
  width: 22px;
  height: 22px;
  display: flex;
  align-items: center;
  justify-content: center;
  border: none;
  border-radius: 50%;
  background: rgba(0, 0, 0, 0.04);
  color: var(--text-muted);
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.2s, background 0.2s;
  flex-shrink: 0;
}

.sub-answer-card:hover .sub-answer-del {
  opacity: 1;
}

.sub-answer-del:hover {
  background: rgba(255, 59, 48, 0.1);
  color: #ff3b30;
}

/* 预览区多小题答案 */
.paper-sub-answer {
  display: flex;
  align-items: flex-start;
  gap: 4px;
  margin-bottom: 4px;
}

.paper-sub-num {
  font-weight: 700;
  flex-shrink: 0;
}

.icon-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  align-self: center;
  width: 30px;
  height: 30px;
  flex-shrink: 0;
  border: 1px solid var(--border-color);
  background: var(--bg-card);
  color: var(--text-muted);
  border-radius: 8px;
  cursor: pointer;
  transition: var(--transition-fast);
}

.icon-btn:hover {
  border-color: var(--danger);
  color: var(--danger);
  background: var(--danger-light);
}

.add-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin-top: 4px;
  padding: 7px 14px;
  border: 1px dashed var(--border-strong);
  background: transparent;
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  border-radius: 8px;
  cursor: pointer;
  transition: var(--transition-fast);
  font-family: inherit;
}

.add-btn:hover {
  border-color: var(--accent);
  color: var(--accent);
  border-style: solid;
  background: var(--accent-light);
}

.add-btn-sm {
  padding: 4px 10px;
  font-size: 12px;
  gap: 4px;
}

/* Radio */
.radio-group {
  display: flex;
  gap: 12px;
}

.radio-label {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  padding: 8px 16px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-secondary);
  transition: border-color 0.18s ease, background-color 0.18s ease, color 0.18s ease;
  user-select: none;
}

.radio-label:hover {
  border-color: var(--border-strong);
}

.radio-label.checked {
  border-color: var(--accent);
  background: var(--accent-light);
  color: var(--accent);
}

.radio-label input {
  margin: 0;
  accent-color: var(--accent);
}

/* 单选/多选精简分段控制器（答案区内） */
.seg-toggle {
  display: inline-flex;
  align-items: center;
  margin-left: 8px;
  gap: 2px;
  background: var(--bg-input);
  border-radius: var(--radius-sm);
  padding: 2px;
}

.seg-btn {
  padding: 2px 8px;
  font-size: 11px;
  font-weight: 500;
  border: none;
  background: transparent;
  color: var(--text-muted);
  cursor: pointer;
  border-radius: calc(var(--radius-sm) - 1px);
  transition: background-color 0.15s ease, color 0.15s ease;
  font-family: inherit;
  line-height: 1.5;
}

.seg-btn.active {
  background: var(--accent);
  color: #fff;
}

/* ============ 高级设置折叠 ============ */
.advanced-section {
  width: 100%;
  margin-top: 6px;
  border: 1px solid var(--border-color);
  border-radius: var(--radius-sm);
  overflow: hidden;
  background: var(--bg-input);
}

.advanced-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  width: 100%;
  padding: 12px 16px;
  background: none;
  border: none;
  font-size: 13px;
  font-weight: 600;
  color: var(--text-secondary);
  cursor: pointer;
  transition: var(--transition-fast);
  font-family: inherit;
}

.advanced-header:hover {
  background: var(--bg-hover);
}

.advanced-title {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}

.collapse-arrow {
  transition: transform 0.2s ease;
  transform: rotate(-90deg);
  color: var(--text-muted);
  display: inline-flex;
}

.collapse-arrow.open {
  transform: rotate(0deg);
}

.advanced-body {
  padding: 4px 16px 16px;
  border-top: 1px solid var(--border-color);
}

.form-grid-2 {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 16px;
  margin-top: 12px;
}

.reviewer-checkboxes {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-height: 120px;
  overflow-y: auto;
}

.reviewer-item {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  cursor: pointer;
  color: var(--text-secondary);
}

.reviewer-item input[type="checkbox"] {
  width: auto;
  accent-color: var(--accent);
}

.hint-line {
  margin-top: 6px;
}

/* ============ 试卷化预览 ============ */

/* 骨架屏 - 同试卷卡片样式 */
.preview-skeleton {
  background: #ffffff;
  border-radius: 8px;
  padding: 32px 28px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.06);
  border: 1px solid rgba(0, 0, 0, 0.04);
}

[data-theme='dark'] .preview-skeleton {
  background: #1c1c1e;
  border-color: rgba(255, 255, 255, 0.06);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
}

.skeleton-line {
  height: 14px;
  border-radius: 6px;
  background: linear-gradient(90deg, var(--bg-input) 25%, var(--bg-hover) 50%, var(--bg-input) 75%);
  background-size: 200% 100%;
  animation: skeleton-shimmer 1.5s ease-in-out infinite;
  margin-bottom: 10px;
}

.skeleton-title { width: 35%; height: 20px; margin-bottom: 20px; }
.skeleton-text { width: 100%; }
.skeleton-short { width: 70%; }
.skeleton-opt { width: 45%; height: 16px; }
.skeleton-answer { width: 30%; height: 16px; }
.skeleton-gap { height: 16px; }

@keyframes skeleton-shimmer {
  0% { background-position: 200% 0; }
  100% { background-position: -200% 0; }
}

/* 试卷卡片 - 悬浮纸张效果 */
.paper-card {
  background: #ffffff;
  border-radius: 8px;
  padding: 24px 28px;
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.06);
  border: 1px solid rgba(0, 0, 0, 0.04);
}

[data-theme='dark'] .paper-card {
  background: #1c1c1e;
  border-color: rgba(255, 255, 255, 0.06);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
}

.paper-card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 16px;
  padding-bottom: 12px;
  border-bottom: 1px solid #f0f0f0;
}

[data-theme='dark'] .paper-card-header {
  border-bottom-color: rgba(255, 255, 255, 0.06);
}

.paper-type-badge {
  font-size: 13px;
  font-weight: 600;
  color: var(--accent);
}

.paper-difficulty {
  display: flex;
  gap: 1px;
}

.paper-star {
  color: #d1d1d6;
  transition: color 0.2s;
}

.paper-star.active {
  color: #ff9500;
}

.paper-stem {
  font-size: 14px;
  line-height: 1.8;
  color: #1d1d1f;
  margin-bottom: 14px;
  word-break: break-word;
  font-family: var(--font-cn-isolated);
}

[data-theme='dark'] .paper-stem {
  color: #f5f5f7;
}

.paper-options {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 24px;
  margin-bottom: 14px;
}

/* 4列横排 — 短选项紧凑排列 */
.paper-options-4col {
  grid-template-columns: repeat(4, 1fr);
  gap: 12px;
}

/* 2列双排 — 默认布局 */
.paper-options-2col {
  grid-template-columns: repeat(2, 1fr);
  gap: 12px 24px;
}

/* 1列竖排 — 长选项或含图片 */
.paper-options-1col {
  grid-template-columns: 1fr;
  gap: 8px;
}

/* 兼容旧类名 */
.paper-options-single {
  grid-template-columns: 1fr;
}

.paper-opt {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  font-size: 13px;
  line-height: 1.7;
  color: #3a3a3c;
  padding: 4px 0;
  font-family: var(--font-cn-isolated);
}

.paper-opt.correct {
  color: var(--accent);
}

[data-theme='dark'] .paper-opt {
  color: #d1d1d6;
}

.paper-opt-letter {
  font-weight: 600;
  flex-shrink: 0;
}

/* 选项内图片样式 */
.paper-opt img.latex-img {
  max-height: 80px;
  width: auto;
  display: inline-block;
  vertical-align: middle;
  margin: 4px 0;
  border-radius: 4px;
}

/* 答案/解析区块 */
.paper-answer-block {
  background: #f5f5f7;
  border-radius: 8px;
  padding: 12px 16px;
  margin-top: 10px;
}

[data-theme='dark'] .paper-answer-block {
  background: rgba(255, 255, 255, 0.04);
}

.paper-answer-label {
  font-size: 12px;
  font-weight: 600;
  color: var(--text-muted);
  text-transform: uppercase;
  letter-spacing: 0.04em;
  margin-bottom: 4px;
}

.paper-answer-content {
  font-size: 13px;
  line-height: 1.7;
  color: var(--text-primary);
  font-family: var(--font-cn-isolated);
}

.paper-correct-answer {
  font-weight: 700;
  font-size: 16px;
  color: var(--accent);
}

.paper-muted {
  color: var(--text-muted);
}

/* 属性面板 — 左右双栏布局 */
.attr-panel {
  display: flex;
  min-height: 340px;
  gap: 0;
}

/* 左侧分类导航 */
.attr-panel-nav {
  flex: 0 0 25%;
  max-width: 160px;
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 0;
  border-right: 1px solid var(--border-color);
  margin-right: -1px;
}

.attr-nav-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 16px;
  border: none;
  background: transparent;
  cursor: pointer;
  font-size: 14px;
  color: var(--text-secondary);
  text-align: left;
  transition: all 0.15s;
  font-family: inherit;
  border-radius: 0;
  position: relative;
}

.attr-nav-item:hover {
  background: var(--accent-light);
  color: var(--accent);
}

.attr-nav-item.active {
  color: var(--accent);
  font-weight: 600;
  background: var(--accent-light);
}

.attr-nav-item.active::before {
  content: '';
  position: absolute;
  left: 0;
  top: 6px;
  bottom: 6px;
  width: 3px;
  border-radius: 0 3px 3px 0;
  background: var(--accent);
}

.attr-nav-badge {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 18px;
  height: 18px;
  padding: 0 5px;
  border-radius: 9px;
  background: var(--accent);
  color: #fff;
  font-size: 11px;
  font-weight: 600;
  margin-left: auto;
}

/* 右侧内容画布 */
.attr-panel-content {
  flex: 1;
  padding: 16px 4px 16px 24px;
  min-width: 0;
}

.attr-canvas {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.attr-canvas-hint {
  font-size: 12px;
  color: var(--text-muted);
  letter-spacing: 0.02em;
}

.attr-dialog-input {
  padding: 8px 12px;
  border-radius: var(--radius-sm);
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-primary);
  font-size: 13px;
  outline: none;
  transition: var(--transition-fast);
  font-family: inherit;
  width: 100%;
  box-sizing: border-box;
}

.attr-dialog-input:focus {
  border-color: var(--accent);
  box-shadow: 0 0 0 3px var(--accent-light);
}

.tag-chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.tag-chips-grid {
  gap: 10px;
}

.tag-chip {
  padding: 6px 14px;
  border-radius: 18px;
  border: 1px solid var(--border-color);
  background: var(--bg-input);
  color: var(--text-secondary);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: var(--transition-fast);
  font-family: inherit;
}

.tag-chip:hover:not(.active) {
  border-color: var(--accent);
  color: var(--accent);
  background: var(--accent-light);
}

.tag-chip.active {
  background: var(--accent);
  color: #fff;
  border-color: var(--accent);
  font-weight: 600;
  box-shadow: 0 1px 3px rgba(0, 122, 255, 0.3);
}

/* ============ Typeahead 联想输入 ============ */
.typeahead-wrap {
  position: relative;
  margin-top: 10px;
}

.typeahead-dropdown {
  position: absolute;
  top: 100%;
  left: 0;
  right: 0;
  z-index: 10;
  background: var(--bg-primary);
  border: 1px solid var(--border);
  border-radius: 8px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.08);
  max-height: 200px;
  overflow-y: auto;
  margin-top: 4px;
}

.typeahead-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  width: 100%;
  padding: 8px 12px;
  border: none;
  background: transparent;
  cursor: pointer;
  font-size: 13px;
  color: var(--text-primary);
  text-align: left;
  transition: background 0.15s;
}

.typeahead-item:hover {
  background: var(--accent-light);
}

.typeahead-count {
  font-size: 11px;
  color: var(--text-muted);
  flex-shrink: 0;
}

.typeahead-create {
  display: block;
  width: 100%;
  margin-top: 6px;
  padding: 8px 12px;
  border: 1px dashed var(--accent);
  border-radius: 8px;
  background: var(--accent-light);
  color: var(--accent);
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: background 0.15s;
}

.typeahead-create:hover {
  background: var(--accent);
  color: #fff;
}

.form-actions {
  display: flex;
  justify-content: flex-end;
  margin-top: 16px;
}

/* ============ 工具类（局部兜底，全局已存在） ============ */
.text-sm {
  font-size: 13px;
}

.text-muted {
  color: var(--text-muted);
}

/* ============ 响应式 ============ */
@media (max-width: 1500px) {
  .main-content {
    flex-direction: column;
  }
  .edit-col {
    flex: none;
    width: 100%;
  }
  .preview-col {
    flex: none;
    width: 100%;
    min-height: 400px;
  }
  /* meta-bar 始终保持单行不换行，仅横向滚动 */
}

@media (max-width: 768px) {
  .choice-grid {
    grid-template-columns: 1fr;
  }
  .blank-wrap {
    flex-direction: column;
    align-items: stretch;
  }
  .blank-input {
    width: 100% !important;
  }
}

@media (max-width: 640px) {
  .edit-page {
    padding: 12px;
  }
  /* meta-bar 始终保持单行不换行，仅横向滚动 */
  .form-grid-2 {
    grid-template-columns: 1fr;
  }
  .top-bar-left,
  .top-bar-right {
    flex-wrap: wrap;
  }
}

/* ===== AI 智能识别弹窗 ===== */
.ai-dialog-body {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.ai-mode-tabs {
  display: flex;
  gap: 4px;
  border-bottom: 2px solid var(--border);
  padding-bottom: 0;
}

.ai-mode-tabs button {
  padding: 8px 16px;
  background: none;
  border: none;
  border-bottom: 2px solid transparent;
  margin-bottom: -2px;
  font-size: 14px;
  color: var(--text-secondary);
  cursor: pointer;
  transition: all 0.2s;
}

.ai-mode-tabs button:hover {
  color: var(--text-primary);
}

.ai-mode-tabs button.active {
  color: var(--purple);
  border-bottom-color: var(--purple);
  font-weight: 600;
}

.ai-prompt-section {
  background: var(--bg-secondary, var(--bg-input));
  border: 1px solid var(--border);
  border-radius: var(--radius);
  padding: 12px;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.ai-prompt-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.ai-prompt-title {
  font-size: 13px;
  font-weight: 600;
  color: var(--text-primary);
}

.ai-prompt-preview {
  font-family: 'SF Mono', 'Fira Code', 'Consolas', monospace;
  font-size: 12px;
  line-height: 1.5;
  color: var(--text-secondary);
  background: var(--bg-input);
  border-radius: 6px;
  padding: 10px;
  max-height: 200px;
  overflow-y: auto;
  white-space: pre-wrap;
  word-break: break-word;
}

.ai-steps {
  font-size: 12px;
  color: var(--text-secondary);
  line-height: 1.5;
}

.ai-hint {
  font-size: 13px;
  color: var(--text-secondary);
  line-height: 1.5;
}

.ai-textarea {
  width: 100%;
  min-height: 200px;
  padding: 12px;
  border: 1px solid var(--border);
  border-radius: var(--radius);
  font-size: 14px;
  font-family: inherit;
  resize: vertical;
  background: var(--bg-input);
  color: var(--text-primary);
  line-height: 1.6;
}

.ai-textarea:focus {
  outline: none;
  border-color: var(--purple);
}

.ai-error {
  padding: 10px 12px;
  background: var(--danger-light);
  color: var(--danger);
  border-radius: var(--radius);
  font-size: 13px;
}

.ai-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.ai-result-meta {
  display: flex;
  align-items: center;
  gap: 10px;
}

.ai-result-type {
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
}

.ai-warnings {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.ai-warning-item {
  font-size: 12px;
  color: var(--warning);
  background: var(--warning-light);
  padding: 6px 10px;
  border-radius: var(--radius);
}

.ai-result-preview {
  max-height: 400px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.ai-preview-block {
  border-left: 3px solid var(--purple-light);
  padding-left: 12px;
}

.ai-preview-label {
  font-size: 12px;
  font-weight: 700;
  color: var(--purple);
  margin-bottom: 4px;
  text-transform: uppercase;
  letter-spacing: 0.05em;
}

.ai-preview-content {
  font-size: 14px;
  color: var(--text-primary);
  line-height: 1.6;
}

.ai-preview-option {
  font-size: 14px;
  color: var(--text-primary);
  padding: 2px 0;
}

.ai-opt-label {
  font-weight: 700;
  margin-right: 4px;
}

.ai-preview-analysis {
  font-size: 13px;
  color: var(--text-secondary);
  padding: 6px 0;
}

/* ===== AI 痕迹高亮（紫色呼吸边框） ===== */
@keyframes ai-breathe {
  0%, 100% {
    box-shadow: 0 0 0 2px var(--purple);
  }
  50% {
    box-shadow: 0 0 8px 2px var(--purple-light);
  }
}

.ai-highlight {
  animation: ai-breathe 2s ease-in-out infinite;
  border-radius: var(--radius);
  transition: box-shadow 0.5s ease;
}
</style>
