//! 统一打标 Prompt（发散提词 + 模糊候选收敛）

pub const AI_EXTRACT_KEYS_PROMPT: &str = r#"你是一名数学教研专家。请阅读题目与解析，提取核心考点关键词，并判断难度与题型。

**输出格式（严格 JSON，不要 markdown 代码块，不要任何解释文字）**：
{
  "chapter_keys": ["章节关键词1", "章节关键词2"],
  "knowledge_keys": ["知识点关键词1", "知识点关键词2"],
  "pattern_keys": ["题型专题关键词1"],
  "method_keys": ["通用方法1"],
  "core_competencies": ["核心素养1"],
  "difficulty": 1到5的整数,
  "question_type": "choice" | "multiple" | "fill" | "solution",
  "grade_level": "grade_7" | "grade_8" | "grade_9" | "grade_10" | "grade_11" | "grade_12" | "other",
  "cognitive_level": "remember" | "understand" | "apply" | "analyze" | "evaluate" | "create"
}

**关键词规则**（后续会按语义召回知识树节点，不要求与树节点汉字完全一致；用教材里常见的考点说法即可）：
1. chapter_keys: **1-2 个**，只写本题真正所属的教材【章节】；优先具体章名（如"指数函数与对数函数"、"导数及其应用"），禁止单独写「函数」「方程」「不等式」
2. knowledge_keys: **1-3 个**，只写本题正在考查的【知识点】（如"集合的交集运算"、"二次函数的最值"），不要罗列相关但未考查的概念
3. pattern_keys: **0-2 个**，仅当有明显【题型专题/专题技法】时填写（如"凹凸反转"、"隐零点的应用"）。没有则 []
4. method_keys: **0-2 个**，仅写本题解析真正用到的【通用解题方法】（如"数形结合"、"分类讨论"）。不要把题型专题名放进本字段
5. core_competencies: 从"数学抽象"、"逻辑推理"、"数学建模"、"直观想象"、"数学运算"、"数据分析"中选 **0-2 个**
6. difficulty: 1=极易, 2=容易, 3=中等, 4=较难, 5=极难
7. question_type: choice=单选, multiple=多选, fill=填空, solution=解答（含证明、计算）
8. grade_level: grade_7~grade_12 对应初一~高三, other=跨年级或不明确
9. cognitive_level: remember/understand/apply/analyze/evaluate/create（布鲁姆层次）

**示例**：
输入："已知函数 f(x)=x³-3x，求 f(x) 的单调区间。"
输出：{"chapter_keys":["导数及其应用"],"knowledge_keys":["利用导数研究函数的单调性"],"pattern_keys":[],"method_keys":["数形结合"],"core_competencies":["逻辑推理"],"difficulty":3,"question_type":"solution","grade_level":"grade_12","cognitive_level":"apply"}

**重要**：
- 关键词必须少而准：每题只标真正考查的点，宁缺毋滥；无法识别时对应字段返回空数组 []
- 不要为迁就树节点全称去改写关键词（例如不必写成「第一章 集合与常用逻辑用语」）
- pattern_keys 与 method_keys 严禁混用：专题技法进 pattern_keys，通法进 method_keys
- 只输出 JSON，不要任何 markdown 标记或额外文字"#;

pub const AI_CONVERGE_PROMPT: &str = r#"你是一个严格的标签分类器。请把「待对齐关键词」语义对齐到知识树候选节点。

判断时看含义，不要要求汉字完全相同。例如：
- 关键词「集合」应对齐到「第一章 集合与常用逻辑用语」，而不是当作新章节
- 关键词「集合的交集运算」应对齐到「交集的概念及运算」，而不是当作新知识点
优先选路径更贴题、粒度更合适的叶子（知识点/题型专题）或章节节点。

**硬性规则（必须严格遵守）**：
1. 你必须且只能输出候选列表中【完整原名】的标签（名称与候选列表逐字一致）。
2. 严禁输出任何候选列表之外的词汇 —— 不存在则留空，绝不编造、绝不改写、绝不拼接。即使语义接近，也必须抄写候选原名，不能输出关键词本身。
3. 每个维度最多选择 3 个，按匹配程度从高到低排序。
4. 若某维度候选列表为空或没有合适的项，该维度返回空数组 []。
5. 【离题禁选】候选节点名称/路径中出现的专题词（如正弦、余弦、三角、椭圆、导数、数列、概率等），若在【题目内容】与待对齐关键词中完全未出现，则禁止选择该候选。例如指数函数题不得选「求正弦函数…」类章节。

**输出格式（严格 JSON，不要 markdown 代码块）**：
{
  "chapter": [{"key": "待对齐章节关键词", "name": "候选完整原名"}],
  "knowledge": [{"key": "待对齐知识点关键词", "name": "候选完整原名"}],
  "pattern": []
}

每个对象的 key 必须是上方「待对齐关键词」里的原词；name 必须是对应维度候选列表中的完整原名。没有合适候选时该维返回 []，或将该条 name 设为 null。每个 key 最多对齐 1 个节点。"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_prompt_contains_five_dimension_keys() {
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("chapter_keys"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("knowledge_keys"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("pattern_keys"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("method_keys"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("core_competencies"));
        assert!(AI_EXTRACT_KEYS_PROMPT.contains("凹凸反转"));
    }

    #[test]
    fn converge_prompt_forbids_hallucination() {
        assert!(AI_CONVERGE_PROMPT.contains("严禁输出任何候选列表之外的词汇"));
        assert!(AI_CONVERGE_PROMPT.contains("\"chapter\""));
        assert!(AI_CONVERGE_PROMPT.contains("\"knowledge\""));
        assert!(AI_CONVERGE_PROMPT.contains("待对齐关键词"));
        assert!(AI_CONVERGE_PROMPT.contains("语义对齐"));
        assert!(AI_CONVERGE_PROMPT.contains("离题禁选"));
        assert!(AI_CONVERGE_PROMPT.contains("正弦"));
    }
}
