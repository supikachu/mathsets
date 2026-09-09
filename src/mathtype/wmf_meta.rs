//! Placeable WMF 几何与 MathType 基线注释解析
//!
//! `MTGetLastDimension` 在并发转换时可能读到陈旧值；WMF 头与 MFCOMMENT
//! 与预览图元绑定，导出对齐应以它们为准。

/// MathType Toolbar/WMF 路径仍可能写出 ``MT Extra``；OLE 已是 Euclid Extra 时
/// 需改写 CREATEFONTINDIRECT，否则 Word 预览与双击编辑器不一致。
pub fn rewrite_wmf_mt_extra_to_euclid(wmf: &[u8]) -> Vec<u8> {
    const META_CREATEFONTINDIRECT: u16 = 0x02FB;
    let old = b"MT Extra";
    let new = b"Euclid Extra";
    if wmf.len() < 40 {
        return wmf.to_vec();
    }
    let mut out = wmf.to_vec();
    let mut idx = if wmf.len() >= 4 && wmf[0..4] == [0xD7, 0xCD, 0xC6, 0x9A] {
        22
    } else {
        0
    };
    idx += 18; // METAHEADER
    let mut changed = false;
    while idx + 6 <= out.len() {
        let size = u32::from_le_bytes([out[idx], out[idx + 1], out[idx + 2], out[idx + 3]]) as usize;
        let func = u16::from_le_bytes([out[idx + 4], out[idx + 5]]);
        if size < 3 {
            break;
        }
        let nbytes = size * 2;
        if idx + nbytes > out.len() {
            break;
        }
        if func == META_CREATEFONTINDIRECT {
            let face_off = idx + 6 + 18;
            let face_end = face_off + 32;
            if face_end <= idx + nbytes {
                let face = {
                    let raw = &out[face_off..face_end];
                    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
                    raw[..end].to_vec()
                };
                if face == old {
                    let mut padded = vec![0u8; 32];
                    padded[..new.len()].copy_from_slice(new);
                    out[face_off..face_end].copy_from_slice(&padded);
                    changed = true;
                }
            }
        }
        idx += nbytes;
    }
    if changed {
        out
    } else {
        wmf.to_vec()
    }
}

/// 从 Aldus Placeable 头读取宽高（pt）
pub fn wmf_size_pt(wmf: &[u8]) -> Option<(f64, f64)> {
    if wmf.len() < 22 || wmf[0..4] != [0xD7, 0xCD, 0xC6, 0x9A] {
        return None;
    }
    let left = i16::from_le_bytes([wmf[6], wmf[7]]) as i32;
    let top = i16::from_le_bytes([wmf[8], wmf[9]]) as i32;
    let right = i16::from_le_bytes([wmf[10], wmf[11]]) as i32;
    let bottom = i16::from_le_bytes([wmf[12], wmf[13]]) as i32;
    let mut inch = u16::from_le_bytes([wmf[14], wmf[15]]) as f64;
    if inch < 1.0 {
        inch = 1440.0;
    }
    let width_pt = (right - left).unsigned_abs() as f64 / inch * 72.0;
    let height_pt = (bottom - top).unsigned_abs() as f64 / inch * 72.0;
    if width_pt < 1.0 || height_pt < 1.0 {
        return None;
    }
    Some((width_pt.min(468.0), height_pt.min(300.0)))
}

/// 从 MathType MFCOMMENT（`MathType\0\0` + i16，单位 1/16 pt）读基线（距底边，pt）
pub fn wmf_baseline_pt(wmf: &[u8]) -> Option<f64> {
    let magic: &[u8] = b"MathType\0\0";
    let idx = find_slice(wmf, magic)?;
    if idx + 12 > wmf.len() {
        return None;
    }
    let raw = i16::from_le_bytes([wmf[idx + 10], wmf[idx + 11]]);
    Some((raw as f64 / 16.0 * 10000.0).round() / 10000.0)
}

/// 用 WMF 内嵌度量覆盖 API 返回值（有则优先）
///
/// 返回 `(baseline_offset_pt, width_pt, height_pt)`。
/// 同时把 WMF 里的 ``MT Extra`` 脸名改成 ``Euclid Extra``（就地写回调用方传入的缓冲
/// 需配合 [`rewrite_wmf_mt_extra_to_euclid`]）。
pub fn prefer_wmf_metrics(
    wmf: &[u8],
    baseline_offset_pt: f64,
    width_pt: f64,
    height_pt: f64,
) -> (f64, f64, f64) {
    let (w, h) = match wmf_size_pt(wmf) {
        Some(size) => size,
        None => (width_pt.max(1.0), height_pt.max(1.0)),
    };
    let bl = wmf_baseline_pt(wmf)
        .unwrap_or(baseline_offset_pt)
        .max(0.0)
        .min(h);
    (bl, w, h)
}

fn find_slice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_placeable_size() {
        let mut wmf = vec![0u8; 22];
        wmf[0..4].copy_from_slice(&[0xD7, 0xCD, 0xC6, 0x9A]);
        wmf[10] = 0xA0; // right = 1440
        wmf[11] = 0x05;
        wmf[12] = 0xD0; // bottom = 720
        wmf[13] = 0x02;
        wmf[14] = 0xA0; // inch = 1440
        wmf[15] = 0x05;
        let (w, h) = wmf_size_pt(&wmf).unwrap();
        assert!((w - 72.0).abs() < 0.01);
        assert!((h - 36.0).abs() < 0.01);
    }

    #[test]
    fn parses_mathtype_baseline_comment() {
        let mut wmf = vec![0u8; 40];
        wmf[0..4].copy_from_slice(&[0xD7, 0xCD, 0xC6, 0x9A]);
        wmf[22..32].copy_from_slice(b"MathType\0\0");
        // 12.0 pt = 192 / 16
        wmf[32] = 192;
        wmf[33] = 0;
        assert!((wmf_baseline_pt(&wmf).unwrap() - 12.0).abs() < 0.01);
    }

    #[test]
    fn prefer_overrides_stale_api_metrics() {
        let mut wmf = vec![0u8; 40];
        wmf[0..4].copy_from_slice(&[0xD7, 0xCD, 0xC6, 0x9A]);
        wmf[10] = 0x80; // right = 384
        wmf[11] = 0x01;
        wmf[12] = 0xC0; // bottom = 448
        wmf[13] = 0x01;
        wmf[14] = 0x00; // inch = 2304
        wmf[15] = 0x09;
        wmf[22..32].copy_from_slice(b"MathType\0\0");
        wmf[32] = 48; // 3.0 pt
        wmf[33] = 0;
        let (bl, w, h) = prefer_wmf_metrics(&wmf, 5.0, 27.0, 16.0);
        assert!((bl - 3.0).abs() < 0.01, "bl={bl}");
        assert!((w - 12.0).abs() < 0.01, "w={w}");
        assert!((h - 14.0).abs() < 0.01, "h={h}");
    }
}
