//! MinerU `*_content_list.json` → LayoutDocument
//!
//! bbox 为页宽高归一化到约 0–1000；`img_path` 可按 zip 落盘后的 URL 表改写。

use super::{BBox, BlockKind, LayoutBlock, LayoutDocument, LayoutSource};
use serde_json::Value;
use std::collections::HashMap;

/// 解析 content_list JSON。根可以是数组，或带 `content_list` 字段的对象。
pub fn parse_mineru_content_list(
    raw: &str,
    image_urls: &HashMap<String, String>,
) -> Result<LayoutDocument, String> {
    let v: Value = serde_json::from_str(raw).map_err(|e| format!("content_list JSON: {e}"))?;
    let arr = match &v {
        Value::Array(a) => a.clone(),
        Value::Object(o) => o
            .get("content_list")
            .or_else(|| o.get("data"))
            .and_then(|x| x.as_array())
            .cloned()
            .ok_or_else(|| "content_list 根对象缺少数组".to_string())?,
        _ => return Err("content_list 根类型不是数组或对象".into()),
    };

    let mut blocks = Vec::with_capacity(arr.len());
    for (i, item) in arr.iter().enumerate() {
        let Some(obj) = item.as_object() else {
            continue;
        };
        let ty = obj
            .get("type")
            .and_then(|x| x.as_str())
            .unwrap_or("text")
            .to_ascii_lowercase();
        let page = obj
            .get("page_idx")
            .and_then(|x| x.as_u64())
            .or_else(|| obj.get("page_idx").and_then(|x| x.as_i64()).map(|n| n.max(0) as u64))
            .unwrap_or(0) as u32;
        let bbox = obj
            .get("bbox")
            .and_then(|b| b.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|n| n.as_f64().or_else(|| n.as_i64().map(|i| i as f64)))
                    .collect::<Vec<_>>()
            })
            .and_then(|xy| BBox::from_xyxy(&xy));
        let text_level = obj.get("text_level").and_then(|x| x.as_u64()).unwrap_or(0);
        let kind = kind_from_mineru(&ty, text_level);
        let img_raw = obj
            .get("img_path")
            .or_else(|| obj.get("image_path"))
            .and_then(|x| x.as_str())
            .map(normalize_img_key);
        let image_url = img_raw.as_ref().and_then(|k| {
            image_urls
                .get(k)
                .cloned()
                .or_else(|| image_urls.get(&format!("images/{k}")).cloned())
                .or_else(|| Some(rewrite_or_keep(k, image_urls)))
        });
        let text = text_from_item(obj, image_url.as_deref());
        if text.trim().is_empty() && image_url.is_none() && kind.is_noise() {
            // 仍保留页眉页脚，供过滤
        }
        blocks.push(LayoutBlock {
            page,
            order: i as u32,
            kind,
            text,
            bbox,
            image_url,
        });
    }

    Ok(LayoutDocument {
        source: LayoutSource::Mineru,
        blocks,
    })
}

fn rewrite_or_keep(key: &str, map: &HashMap<String, String>) -> String {
    let n = key.trim_start_matches("./").replace('\\', "/");
    map.get(&n)
        .or_else(|| map.get(key))
        .cloned()
        .unwrap_or_else(|| n)
}

fn normalize_img_key(p: &str) -> String {
    p.trim_start_matches("./")
        .trim_start_matches("../")
        .replace('\\', "/")
}

fn kind_from_mineru(ty: &str, text_level: u64) -> BlockKind {
    match ty {
        "header" | "footer" | "page_number" | "aside_text" | "page_footnote" => {
            BlockKind::HeaderFooter
        }
        "image" | "chart" => BlockKind::Image,
        "table" => BlockKind::Table,
        "equation" => BlockKind::Formula,
        "title" => BlockKind::Title,
        _ if text_level == 1 => BlockKind::Title,
        _ => BlockKind::Text,
    }
}

fn text_from_item(obj: &serde_json::Map<String, Value>, image_url: Option<&str>) -> String {
    if let Some(t) = obj.get("text").and_then(|x| x.as_str()) {
        if !t.trim().is_empty() {
            return t.to_string();
        }
    }
    if let Some(t) = obj.get("table_body").and_then(|x| x.as_str()) {
        if !t.trim().is_empty() {
            return t.to_string();
        }
    }
    if let Some(t) = obj.get("code_body").and_then(|x| x.as_str()) {
        if !t.trim().is_empty() {
            return t.to_string();
        }
    }
    if let Some(caps) = obj.get("image_caption").and_then(|x| x.as_array()) {
        let cap: Vec<&str> = caps.iter().filter_map(|x| x.as_str()).collect();
        if let Some(url) = image_url {
            if cap.is_empty() {
                return format!("![]({url})");
            }
            return format!("![{}]({url})", cap.join(" "));
        }
        if !cap.is_empty() {
            return cap.join(" ");
        }
    }
    if let Some(url) = image_url {
        return format!("![]({url})");
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_flat_array() {
        let raw = r#"[
            {"type":"header","text":"第 1 页","bbox":[10,10,900,40],"page_idx":0},
            {"type":"text","text":"1. 已知集合 A","bbox":[80,120,480,180],"page_idx":0,"text_level":0},
            {"type":"image","img_path":"images/a.jpg","bbox":[80,200,480,400],"page_idx":0}
        ]"#;
        let mut urls = HashMap::new();
        urls.insert(
            "images/a.jpg".into(),
            "/uploads/questions/uuid.jpg".into(),
        );
        let doc = parse_mineru_content_list(raw, &urls).unwrap();
        assert_eq!(doc.source, LayoutSource::Mineru);
        assert_eq!(doc.blocks.len(), 3);
        assert_eq!(doc.blocks[0].kind, BlockKind::HeaderFooter);
        assert_eq!(doc.blocks[1].text, "1. 已知集合 A");
        assert_eq!(
            doc.blocks[2].image_url.as_deref(),
            Some("/uploads/questions/uuid.jpg")
        );
        assert!(doc.blocks[2].text.contains("/uploads/questions/uuid.jpg"));
    }
}
