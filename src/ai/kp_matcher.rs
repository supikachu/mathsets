use crate::models::question::KnowledgePointTreeNode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 知识点匹配结果
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct KpMatch {
    /// AI 返回的原始名称
    pub ai_name: String,
    /// 匹配到的知识点 ID
    pub matched_id: Option<Uuid>,
    /// 匹配到的知识点名称
    pub matched_name: Option<String>,
    /// 0.0-1.0 相似度
    pub score: f32,
}

/// 对 AI 返回的知识点名称，在树中找最佳匹配
pub fn match_knowledge_points(
    ai_names: &[String],
    tree: &[KnowledgePointTreeNode],
) -> Vec<KpMatch> {
    let flat = flatten_tree(tree);
    ai_names
        .iter()
        .map(|name| {
            let best = find_best_match(name, &flat);
            KpMatch {
                ai_name: name.clone(),
                matched_id: best.as_ref().map(|(id, _, _)| *id),
                matched_name: best.as_ref().map(|(_, n, _)| n.clone()),
                score: best.as_ref().map(|(_, _, s)| *s).unwrap_or(0.0),
            }
        })
        .collect()
}

/// 递归展平知识点树
fn flatten_tree(tree: &[KnowledgePointTreeNode]) -> Vec<(Uuid, String)> {
    let mut result = Vec::new();
    for node in tree {
        result.push((node.id, node.name.clone()));
        result.extend(flatten_tree(&node.children));
    }
    result
}

/// Levenshtein 距离归一化为 0-1 相似度
fn find_best_match(name: &str, flat: &[(Uuid, String)]) -> Option<(Uuid, String, f32)> {
    let name_lower = name.to_lowercase();
    flat.iter()
        .map(|(id, kp_name)| {
            let kp_lower = kp_name.to_lowercase();
            let dist = levenshtein(&name_lower, &kp_lower);
            // 用字符数（而非字节数）归一化，避免中文 UTF-8 多字节导致分数虚高
            let max_len = name_lower.chars().count().max(kp_lower.chars().count()).max(1) as f32;
            let score = 1.0 - (dist as f32 / max_len);
            (*id, kp_name.clone(), score)
        })
        .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
        .filter(|(_, _, s)| *s > 0.3) // 低于 0.3 视为不匹配
}

/// Levenshtein 编辑距离
fn levenshtein(a: &str, b: &str) -> usize {
    let a_chars: Vec<char> = a.chars().collect();
    let b_chars: Vec<char> = b.chars().collect();
    let m = a_chars.len();
    let n = b_chars.len();

    if m == 0 {
        return n;
    }
    if n == 0 {
        return m;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr: Vec<usize> = vec![0; n + 1];

    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a_chars[i - 1] == b_chars[j - 1] {
                0
            } else {
                1
            };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    prev[n]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tree() -> Vec<KnowledgePointTreeNode> {
        vec![KnowledgePointTreeNode {
            id: Uuid::nil(),
            parent_id: None,
            name: "数学".to_string(),
            grade: None,
            sort_order: 0,
            children: vec![KnowledgePointTreeNode {
                id: Uuid::new_v4(),
                parent_id: Some(Uuid::nil()),
                name: "一次函数".to_string(),
                grade: Some("8".to_string()),
                sort_order: 1,
                children: vec![KnowledgePointTreeNode {
                    id: Uuid::new_v4(),
                    parent_id: None,
                    name: "集合的概念".to_string(),
                    grade: None,
                    sort_order: 0,
                    children: vec![],
                }],
            }],
        }]
    }

    #[test]
    fn test_exact_match() {
        let tree = make_tree();
        let matches = match_knowledge_points(&["一次函数".to_string()], &tree);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].score >= 0.95);
        assert_eq!(matches[0].matched_name.as_deref(), Some("一次函数"));
    }

    #[test]
    fn test_fuzzy_match() {
        let tree = make_tree();
        let matches = match_knowledge_points(&["集合".to_string()], &tree);
        assert_eq!(matches.len(), 1);
        // "集合" 应该模糊匹配到 "集合的概念"
        assert!(matches[0].score > 0.3);
        assert_eq!(matches[0].matched_name.as_deref(), Some("集合的概念"));
    }

    #[test]
    fn test_no_match() {
        let tree = make_tree();
        let matches = match_knowledge_points(&["量子力学".to_string()], &tree);
        assert_eq!(matches.len(), 1);
        assert!(matches[0].matched_id.is_none());
    }

    #[test]
    fn test_levenshtein() {
        assert_eq!(levenshtein("", "abc"), 3);
        assert_eq!(levenshtein("abc", "abc"), 0);
        assert_eq!(levenshtein("kitten", "sitting"), 3);
    }
}
