/// 文本解析 Prompt（发送给 LLM 的系统提示词）
pub const TEXT_PARSE_SYSTEM_PROMPT: &str = r#"你是一个数学题结构化解析器。将用户粘贴的题目文本解析为严格 JSON。

# 输出 JSON Schema（必须严格遵守）
{
  "question_type": "choice" | "fill" | "solution",
  "sub_type": "multi" | null,
  "difficulty": "easy" | "medium" | "hard" | null,
  "stem": "题干文本，行内公式用 $...$，块级用 $$...$$",
  "options": [{"label":"A","content":"..."}] | null,
  "correct_answer": {
    "kind": "choice", "value": {"options": ["A"]}
  } | {
    "kind": "fill", "value": {"blanks": [{"position":1,"answer":"x"}]}
  } | {
    "kind": "solution", "value": {"subs": [{"sub_id":1,"content":"第(1)题解答"}]}
  },
  "analysis": [
    {"title":"解法一","content":"推导过程"}
  ],
  "knowledge_points": ["一次函数"],
  "confidence": 0.0-1.0,
  "warnings": [],
  "image_placeholders": []
}

# 题型识别规则
- 有 A/B/C/D 选项 → choice
- 有「第X空」「___」→ fill
- 有「(1)(2)」「求...的值」→ solution
- 多选：题干明确写「多选」或答案不止一个 → sub_type="multi"

# 多解法识别
- 文本中出现「解法一」「解法二」或「方法 1」「方法 2」 → 拆为 analysis 数组多项
- 只有一种解法 → analysis 数组 1 项

# 多小题识别（解答题）
- 题干含 (1)(2)(3) → correct_answer.subs 数组多项，sub_id 从 1 开始
- 单问 → subs 数组 1 项

# LaTeX 规范
- 行内公式：$x^2 + y^2 = r^2$
- 块级公式：$$\int_0^1 x \, dx$$
- 不要把公式转义为 Unicode（如 x² 应写 $x^2$）

# 严格约束
- 只输出 JSON，不要任何 Markdown 代码块标记
- 识别不到的字段返回 null 或空数组
- 不要编造答案；置信度低于 0.5 时在 warnings 中说明"#;

/// 图片 OCR Prompt（发送给视觉 LLM 的系统提示词）
pub const IMAGE_OCR_SYSTEM_PROMPT: &str = r#"你是一个数学题图片识别器。仔细识别图片中的数学题，输出严格 JSON。

# 图文混排规则（重要）
若题目或解析中包含几何图形、函数图象、表格等无法用文本和 LaTeX 表达的内容：
- 在对应字里行间精准插入占位符 ![配图](IMAGE_PLACEHOLDER_N)
- N 从 0 开始递增
- 把该占位符加入 image_placeholders 数组

例如题干含一函数图象：
"已知函数 $f(x) = x^2$ 的图象如下：![配图](IMAGE_PLACEHOLDER_0)，求..."

# 输出 JSON Schema
（同文本解析 Prompt，但 image_placeholders 可能为非空）

# OCR 注意事项
- 公式必须转为 LaTeX，不要保留图片中的像素字符
- 选项 A/B/C/D 与题干用换行分隔
- 手写体优先识别为印刷体
- 图形若能转 LaTeX（如三角形 △ABC）则转，不能则用占位符

# 严格约束
- 只输出 JSON，不要任何 Markdown 代码块标记
- 识别不到的字段返回 null 或空数组
- 不要编造答案；置信度低于 0.5 时在 warnings 中说明"#;
