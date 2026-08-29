//! 集成测试辅助（`cargo test` 使用独立库，避免污染 `DATABASE_URL` 开发库）

use serde_json::{json, Value};

/// 加载 `.env` 并返回 `DATABASE_URL_TEST`。
///
/// 未配置时返回 `None`；集成测试应跳过或失败，**不得**回退到 `DATABASE_URL`。
pub fn database_url() -> Option<String> {
    let _ = dotenvy::dotenv();
    std::env::var("DATABASE_URL_TEST").ok()
}

/// 单叶解答题 `structure`（固定 id，便于 hash 对比）
pub fn solution_structure_json(answer: &str, analysis: &str) -> Value {
    json!({
        "version": 1,
        "parts": [{
            "id": "11111111-1111-4111-8111-111111111111",
            "label": "(1)",
            "stem": "",
            "children": [],
            "answer": answer,
            "analyses": [{"id": "a1", "title": "解法一", "content": analysis}],
            "no_analysis_needed": false,
            "label_dirty": false
        }]
    })
}

/// 图 2：I 下两小问 + 独立 II
pub fn fig2_structure_json() -> Value {
    json!({
        "version": 1,
        "parts": [
            {
                "id": "part-I",
                "label": "I",
                "stem": "若 $f(x)$ 为奇函数",
                "children": [
                    {
                        "id": "part-I-i",
                        "label": "(i)",
                        "stem": "求 $m$",
                        "children": [],
                        "answer": "$m=-1$",
                        "analyses": [{"id": "a-i", "title": "解法一", "content": "由奇函数得 $m=-1$"}],
                        "no_analysis_needed": false,
                        "label_dirty": true
                    },
                    {
                        "id": "part-I-ii",
                        "label": "(ii)",
                        "stem": "求 $a$ 的范围",
                        "children": [],
                        "answer": "$a>0$",
                        "analyses": [{"id": "a-ii", "title": "解法一", "content": "单调性"}],
                        "no_analysis_needed": false,
                        "label_dirty": true
                    }
                ],
                "answer": null,
                "analyses": [],
                "no_analysis_needed": false,
                "label_dirty": true
            },
            {
                "id": "part-II",
                "label": "II",
                "stem": "求 $p(m)$",
                "children": [],
                "answer": "$p(m)=|m|$",
                "analyses": [
                    {"id": "a-ii-1", "title": "解法一", "content": "分类讨论"},
                    {"id": "a-ii-2", "title": "解法二", "content": "绝对值定义"}
                ],
                "no_analysis_needed": false,
                "label_dirty": true
            }
        ]
    })
}

/// 图 3：独立 I + II 下两小问
pub fn fig3_structure_json() -> Value {
    json!({
        "version": 1,
        "parts": [
            {
                "id": "part-I",
                "label": "I",
                "stem": "求 $f(1)$",
                "children": [],
                "answer": "2",
                "analyses": [{"id": "a-I", "title": "解法一", "content": "代入"}],
                "no_analysis_needed": false,
                "label_dirty": true
            },
            {
                "id": "part-II",
                "label": "II",
                "stem": "若 $f(x)$ 为偶函数",
                "children": [
                    {
                        "id": "part-II-i",
                        "label": "(i)",
                        "stem": "求 $m$",
                        "children": [],
                        "answer": "$m=0$",
                        "analyses": [{"id": "a-i", "title": "解法一", "content": "偶函数"}],
                        "no_analysis_needed": false,
                        "label_dirty": true
                    },
                    {
                        "id": "part-II-ii",
                        "label": "(ii)",
                        "stem": "求值域",
                        "children": [],
                        "answer": "$[0,+\\infty)$",
                        "analyses": [{"id": "a-ii", "title": "解法一", "content": "配方"}],
                        "no_analysis_needed": false,
                        "label_dirty": true
                    }
                ],
                "answer": null,
                "analyses": [],
                "no_analysis_needed": false,
                "label_dirty": true
            }
        ]
    })
}
