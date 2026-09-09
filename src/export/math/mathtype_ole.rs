//! MathType OLE `w:object` 片段（Equation.DSMT4 + placeable WMF）

use crate::mathtype::ReadyMathAsset;

/// VML shapetype `_x0000_t75`（整篇文档只应出现一次，避免 Word 叠绘）
pub const SHAPE_TYPE_75: &str = concat!(
    r#"<v:shapetype id="_x0000_t75" coordsize="21600,21600" o:spt="75" o:preferrelative="t" "#,
    r#"path="m@4@5l@4@11@9@11@9@5xe" filled="f" stroked="f">"#,
    r#"<v:stroke joinstyle="miter"/>"#,
    r#"<v:formulas>"#,
    r#"<v:f eqn="if lineDrawn pixelLineWidth 0"/>"#,
    r#"<v:f eqn="sum @0 1 0"/>"#,
    r#"<v:f eqn="sum 0 0 @1"/>"#,
    r#"<v:f eqn="prod @2 1 2"/>"#,
    r#"<v:f eqn="prod @3 21600 pixelWidth"/>"#,
    r#"<v:f eqn="prod @3 21600 pixelHeight"/>"#,
    r#"<v:f eqn="sum @0 0 1"/>"#,
    r#"<v:f eqn="prod @6 1 2"/>"#,
    r#"<v:f eqn="prod @7 21600 pixelWidth"/>"#,
    r#"<v:f eqn="sum @8 21600 0"/>"#,
    r#"<v:f eqn="prod @7 21600 pixelHeight"/>"#,
    r#"<v:f eqn="sum @10 21600 0"/>"#,
    r#"</v:formulas>"#,
    r#"<v:path o:extrusionok="f" gradientshapeok="t" o:connecttype="rect"/>"#,
    r#"<o:lock v:ext="edit" aspectratio="t"/>"#,
    r#"</v:shapetype>"#
);

/// 从 placeable WMF 头读取宽高（pt）；失败则回退资产字段 / 默认值
pub fn estimate_wmf_size_pt(wmf: &[u8], fallback_w: f64, fallback_h: f64) -> (f64, f64) {
    if wmf.len() < 22 || wmf[0..4] != [0xD7, 0xCD, 0xC6, 0x9A] {
        return (fallback_w.max(1.0), fallback_h.max(1.0));
    }
    let left = i16::from_le_bytes([wmf[6], wmf[7]]) as i32;
    let top = i16::from_le_bytes([wmf[8], wmf[9]]) as i32;
    let right = i16::from_le_bytes([wmf[10], wmf[11]]) as i32;
    let bottom = i16::from_le_bytes([wmf[12], wmf[13]]) as i32;
    let mut inch = u16::from_le_bytes([wmf[14], wmf[15]]) as f64;
    if inch < 1.0 {
        inch = 1440.0;
    }
    let mut width_pt = (right - left).unsigned_abs() as f64 / inch * 72.0;
    let mut height_pt = (bottom - top).unsigned_abs() as f64 / inch * 72.0;
    if width_pt < 1.0 || height_pt < 1.0 {
        return (fallback_w.max(1.0), fallback_h.max(1.0));
    }
    width_pt = width_pt.min(468.0);
    height_pt = height_pt.min(300.0);
    (width_pt, height_pt)
}

/// 生成含 `w:object` 的 `w:r`（嵌入式行内 OLE）
///
/// `include_shapetype`：仅第一个公式为 true，避免重复定义 `_x0000_t75` 导致叠绘。
pub fn equation_run(
    asset: &ReadyMathAsset,
    index: usize,
    r_id_img: &str,
    r_id_ole: &str,
    include_shapetype: bool,
) -> String {
    let fw = if asset.width_pt > 0.0 {
        asset.width_pt
    } else {
        36.0
    };
    let fh = if asset.height_pt > 0.0 {
        asset.height_pt
    } else {
        18.0
    };
    let (width, height) = estimate_wmf_size_pt(&asset.wmf, fw, fh);
    let baseline = asset.baseline_offset_pt.max(0.0).min(height);

    let n = index + 1;
    let shape_id = format!("_x0000_i{}", 1025 + index);
    let object_id = format!("_{:08X}", 0x1A2B0000u32.wrapping_add(n as u32));

    // 只用 width/height；基线对齐交给 w:position，避免再加 position:relative;top 导致叠字
    let style = format!("width:{width:.3}pt;height:{height:.3}pt");

    let dxa = ((width * 20.0).round() as i64).max(1);
    let dya = ((height * 20.0).round() as i64).max(1);

    let shape_type = "#_x0000_t75";
    let mut shape = String::new();
    shape.push_str("<v:shape id=\"");
    shape.push_str(&shape_id);
    shape.push_str("\" type=\"");
    shape.push_str(shape_type);
    shape.push_str("\" style=\"");
    shape.push_str(&style);
    shape.push_str("\" o:ole=\"\"><v:imagedata r:id=\"");
    shape.push_str(r_id_img);
    shape.push_str("\" o:title=\"\"/></v:shape>");

    let mut ole = String::new();
    ole.push_str("<o:OLEObject Type=\"Embed\" ProgID=\"Equation.DSMT4\" ShapeID=\"");
    ole.push_str(&shape_id);
    ole.push_str("\" DrawAspect=\"Content\" ObjectID=\"");
    ole.push_str(&object_id);
    ole.push_str("\" r:id=\"");
    ole.push_str(r_id_ole);
    ole.push_str("\"/>");

    let mut run = String::from("<w:r>");
    if baseline > 0.001 {
        // half-points；负值把公式下移，使公式基线与正文对齐
        let half = -((baseline * 2.0).round() as i64);
        run.push_str(&format!(
            r#"<w:rPr><w:position w:val="{half}"/></w:rPr>"#
        ));
    }
    run.push_str(&format!(
        r#"<w:object w:dxaOrig="{dxa}" w:dyaOrig="{dya}">"#
    ));
    if include_shapetype {
        run.push_str(SHAPE_TYPE_75);
    }
    run.push_str(&shape);
    run.push_str(&ole);
    run.push_str("</w:object></w:r>");
    run
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_placeable_wmf_size() {
        // Minimal fake placeable header: key + bbox 0,0,1440,720 at 1440 dpi → 72×36 pt
        let mut wmf = vec![0u8; 22];
        wmf[0..4].copy_from_slice(&[0xD7, 0xCD, 0xC6, 0x9A]);
        wmf[10] = 0xA0; // right = 1440
        wmf[11] = 0x05;
        wmf[12] = 0xD0; // bottom = 720
        wmf[13] = 0x02;
        wmf[14] = 0xA0; // inch = 1440
        wmf[15] = 0x05;
        let (w, h) = estimate_wmf_size_pt(&wmf, 1.0, 1.0);
        assert!((w - 72.0).abs() < 0.01, "w={w}");
        assert!((h - 36.0).abs() < 0.01, "h={h}");
    }
}
