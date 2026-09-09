using System.Globalization;
using System.IO.Compression;
using System.Text;
using System.Xml.Linq;

namespace VisualTeX.WordVsto;

/// <summary>
/// Builds a minimal Word OOXML (.docx) that embeds MathType OLE objects
/// (<c>Equation.DSMT4</c>) with placeable WMF previews — for local verification.
/// </summary>
public static class MathTypeDocxWriter
{
    private static readonly XNamespace W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    private static readonly XNamespace R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    private static readonly XNamespace V = "urn:schemas-microsoft-com:vml";
    private static readonly XNamespace O = "urn:schemas-microsoft-com:office:office";
    private static readonly XNamespace RelPkg = "http://schemas.openxmlformats.org/package/2006/relationships";
    private static readonly XNamespace Ct = "http://schemas.openxmlformats.org/package/2006/content-types";
    private static readonly XNamespace Dc = "http://purl.org/dc/elements/1.1/";
    private static readonly XNamespace DcTerms = "http://purl.org/dc/terms/";
    private static readonly XNamespace Xsi = "http://www.w3.org/2001/XMLSchema-instance";
    private static readonly XNamespace Cp = "http://schemas.openxmlformats.org/package/2006/metadata/core-properties";

    public sealed class EquationPart
    {
        public string Caption { get; set; } = string.Empty;
        public byte[] OleBin { get; set; } = Array.Empty<byte>();
        public byte[] Wmf { get; set; } = Array.Empty<byte>();
        public double? BaselineOffsetPt { get; set; }
        public bool Inline { get; set; } = false;
        public double? WidthPt { get; set; }
        public double? HeightPt { get; set; }
    }

    /// <summary>
    /// MathML strings → ole.bin + WMF → one demo .docx (requires MathType for WMF).
    /// </summary>
    public static void WriteDemoDocxFromMathMl(
        IReadOnlyList<(string caption, string mathMl, bool inline)> equations,
        string docxPath,
        double? fontSizePt = null)
    {
        if (equations is null || equations.Count == 0)
            throw new ArgumentException("At least one equation is required.", nameof(equations));

        var parts = new List<EquationPart>(equations.Count);
        foreach (var (caption, mathMl, inline) in equations)
        {
            var bin = fontSizePt.HasValue
                ? MathTypeOleStorage.CreateStandaloneCompoundFile(mathMl, inline, fontSizePt.Value)
                : MathTypeOleStorage.CreateStandaloneCompoundFile(mathMl, inline);
            var wmfPath = Path.Combine(
                Path.GetTempPath(),
                "mathset-mathtype-ole",
                $"preview-{Guid.NewGuid():N}.wmf");
            try
            {
                Directory.CreateDirectory(Path.GetDirectoryName(wmfPath)!);
                if (fontSizePt.HasValue)
                    MathTypeWmfConverter.WriteWmfFromMathMl(mathMl, wmfPath, inline, fontSizePt.Value);
                else
                    MathTypeWmfConverter.WriteWmfFromMathMl(mathMl, wmfPath, inline);
                var wmfBytes = File.ReadAllBytes(wmfPath);
                parts.Add(new EquationPart
                {
                    Caption = caption,
                    OleBin = bin,
                    Wmf = wmfBytes,
                    Inline = inline,
                    BaselineOffsetPt = EstimateWmfBaselinePt(wmfBytes),
                });
            }
            finally
            {
                try { if (File.Exists(wmfPath)) File.Delete(wmfPath); }
                catch { }
            }
        }

        WriteDocx(parts, docxPath);
    }

    public static void WriteDocx(IReadOnlyList<EquationPart> equations, string docxPath)
    {
        if (equations is null || equations.Count == 0)
            throw new ArgumentException("At least one equation is required.", nameof(equations));
        if (string.IsNullOrWhiteSpace(docxPath))
            throw new ArgumentException("DOCX path is required.", nameof(docxPath));

        var fullPath = Path.GetFullPath(docxPath);
        var dir = Path.GetDirectoryName(fullPath);
        if (!string.IsNullOrEmpty(dir))
            Directory.CreateDirectory(dir);
        if (File.Exists(fullPath))
            File.Delete(fullPath);

        using var fs = File.Create(fullPath);
        using var zip = new ZipArchive(fs, ZipArchiveMode.Create);

        WriteEntry(zip, "[Content_Types].xml", BuildContentTypes(equations.Count));
        WriteEntry(zip, "_rels/.rels", BuildPackageRels());
        WriteEntry(zip, "docProps/core.xml", BuildCoreProps());
        WriteEntry(zip, "word/document.xml", BuildDocumentXml(equations));
        WriteEntry(zip, "word/_rels/document.xml.rels", BuildDocumentRels(equations.Count));

        for (var i = 0; i < equations.Count; i++)
        {
            var n = i + 1;
            WriteBinary(zip, $"word/media/image{n}.wmf", equations[i].Wmf);
            WriteBinary(zip, $"word/embeddings/oleObject{n}.bin", equations[i].OleBin);
        }
    }

    /// <summary>
    /// Creates an OOXML &lt;w:r&gt; containing a MathType OLE equation with
    /// baseline adjustment (&lt;w:position&gt; and VML &lt;v:shape style="...top:..."&gt;).
    /// </summary>
    public static XElement CreateEquationRun(EquationPart part, int index)
    {
        if (part is null) throw new ArgumentNullException(nameof(part));
        var (widthPt, heightPt) = EstimateWmfSizePt(part.Wmf);
        if (part.WidthPt.HasValue && part.WidthPt.Value > 0) widthPt = part.WidthPt.Value;
        if (part.HeightPt.HasValue && part.HeightPt.Value > 0) heightPt = part.HeightPt.Value;

        var baselinePt = part.BaselineOffsetPt ?? EstimateWmfBaselinePt(part.Wmf);
        var n = index + 1;
        var shapeId = $"_x0000_i{1025 + index}";
        var objectId = $"_{unchecked((uint)(0x1A2B0000 + n)):X8}";

        var styleBuilder = new StringBuilder();
        styleBuilder.AppendFormat(CultureInfo.InvariantCulture, "width:{0:0.###}pt;height:{1:0.###}pt", widthPt, heightPt);
        if (baselinePt > 0.001)
        {
            styleBuilder.AppendFormat(CultureInfo.InvariantCulture, ";position:relative;top:{0:0.###}pt", baselinePt);
        }

        var shape = new XElement(
            V + "shape",
            new XAttribute("id", shapeId),
            new XAttribute("type", "#_x0000_t75"),
            new XAttribute("style", styleBuilder.ToString()),
            new XAttribute(O + "ole", ""),
            new XElement(
                V + "imagedata",
                new XAttribute(R + "id", $"rIdImg{n}"),
                new XAttribute(O + "title", "")));

        var ole = new XElement(
            O + "OLEObject",
            new XAttribute("Type", "Embed"),
            new XAttribute("ProgID", MathTypeOleIdentity.CanonicalProgId),
            new XAttribute("ShapeID", shapeId),
            new XAttribute("DrawAspect", "Content"),
            new XAttribute("ObjectID", objectId),
            new XAttribute(R + "id", $"rIdOle{n}"));

        // dxaOrig / dyaOrig: twentieths of a point
        var dxa = Math.Max(1, (int)Math.Round(widthPt * 20));
        var dya = Math.Max(1, (int)Math.Round(heightPt * 20));

        var obj = new XElement(
            W + "object",
            new XAttribute(W + "dxaOrig", dxa.ToString(CultureInfo.InvariantCulture)),
            new XAttribute(W + "dyaOrig", dya.ToString(CultureInfo.InvariantCulture)),
            ShapeType75(),
            shape,
            ole);

        var run = new XElement(W + "r");
        if (baselinePt > 0.001)
        {
            // Word specifies <w:position> in half-points (1/2 pt).
            // Negative value lowers the run so the equation's baseline matches the surrounding text baseline.
            var halfPoints = -(int)Math.Round(baselinePt * 2.0);
            run.Add(new XElement(
                W + "rPr",
                new XElement(W + "position", new XAttribute(W + "val", halfPoints.ToString(CultureInfo.InvariantCulture)))));
        }
        run.Add(obj);
        return run;
    }

    private static string BuildDocumentXml(IReadOnlyList<EquationPart> equations)
    {
        var body = new XElement(W + "body");
        body.Add(new XElement(
            W + "p",
            new XElement(
                W + "r",
                new XElement(
                    W + "t",
                    "mathset MathType OLE smoke test — double-click equations to open in MathType."))));

        for (var i = 0; i < equations.Count; i++)
        {
            var part = equations[i];
            var n = i + 1;
            var run = CreateEquationRun(part, i);

            if (part.Inline)
            {
                // Inline equation: mixed with text in the same paragraph with auto line spacing
                var p = new XElement(
                    W + "p",
                    new XElement(W + "pPr",
                        new XElement(W + "spacing", new XAttribute(W + "lineRule", "auto"))),
                    new XElement(W + "r", new XElement(W + "t", $"{n}. {part.Caption}: 前置正文文字 ")),
                    run,
                    new XElement(W + "r", new XElement(W + "t", " 后续混排正文文字（基线已对齐）。")));
                body.Add(p);
            }
            else
            {
                if (!string.IsNullOrWhiteSpace(part.Caption))
                {
                    body.Add(new XElement(
                        W + "p",
                        new XElement(
                            W + "r",
                            new XElement(W + "t", $"{n}. {part.Caption}"))));
                }

                var p = new XElement(
                    W + "p",
                    new XElement(W + "pPr",
                        new XElement(W + "spacing", new XAttribute(W + "lineRule", "auto"))),
                    run);
                body.Add(p);
            }
        }

        body.Add(new XElement(
            W + "sectPr",
            new XElement(
                W + "pgSz",
                new XAttribute(W + "w", "11906"),
                new XAttribute(W + "h", "16838")),
            new XElement(
                W + "pgMar",
                new XAttribute(W + "top", "1440"),
                new XAttribute(W + "right", "1440"),
                new XAttribute(W + "bottom", "1440"),
                new XAttribute(W + "left", "1440"))));

        var doc = new XDocument(
            new XDeclaration("1.0", "UTF-8", "yes"),
            new XElement(
                W + "document",
                new XAttribute(XNamespace.Xmlns + "w", W),
                new XAttribute(XNamespace.Xmlns + "r", R),
                new XAttribute(XNamespace.Xmlns + "v", V),
                new XAttribute(XNamespace.Xmlns + "o", O),
                body));

        return doc.ToString(SaveOptions.DisableFormatting);
    }

    private static XElement ShapeType75() =>
        new(
            V + "shapetype",
            new XAttribute("id", "_x0000_t75"),
            new XAttribute("coordsize", "21600,21600"),
            new XAttribute(O + "spt", "75"),
            new XAttribute(O + "preferrelative", "t"),
            new XAttribute("path", "m@4@5l@4@11@9@11@9@5xe"),
            new XAttribute("filled", "f"),
            new XAttribute("stroked", "f"),
            new XElement(
                V + "stroke",
                new XAttribute("joinstyle", "miter")),
            new XElement(
                V + "formulas",
                Formula("if lineDrawn pixelLineWidth 0"),
                Formula("sum @0 1 0"),
                Formula("sum 0 0 @1"),
                Formula("prod @2 1 2"),
                Formula("prod @3 21600 pixelWidth"),
                Formula("prod @3 21600 pixelHeight"),
                Formula("sum @0 0 1"),
                Formula("prod @6 1 2"),
                Formula("prod @7 21600 pixelWidth"),
                Formula("sum @8 21600 0"),
                Formula("prod @7 21600 pixelHeight"),
                Formula("sum @10 21600 0")),
            new XElement(V + "path", new XAttribute(O + "extrusionok", "f"), new XAttribute("gradientshapeok", "t"), new XAttribute(O + "connecttype", "rect")),
            new XElement(O + "lock", new XAttribute(V + "ext", "edit"), new XAttribute("aspectratio", "t")));

    private static XElement Formula(string eqn) =>
        new(V + "f", new XAttribute("eqn", eqn));

    private static string BuildDocumentRels(int count)
    {
        var root = new XElement(
            RelPkg + "Relationships",
            new XAttribute("xmlns", RelPkg.NamespaceName));

        for (var i = 1; i <= count; i++)
        {
            root.Add(new XElement(
                RelPkg + "Relationship",
                new XAttribute("Id", $"rIdImg{i}"),
                new XAttribute(
                    "Type",
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"),
                new XAttribute("Target", $"media/image{i}.wmf")));
            root.Add(new XElement(
                RelPkg + "Relationship",
                new XAttribute("Id", $"rIdOle{i}"),
                new XAttribute(
                    "Type",
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/oleObject"),
                new XAttribute("Target", $"embeddings/oleObject{i}.bin")));
        }

        return new XDocument(root).ToString(SaveOptions.DisableFormatting);
    }

    private static string BuildPackageRels()
    {
        var root = new XElement(
            RelPkg + "Relationships",
            new XAttribute("xmlns", RelPkg.NamespaceName),
            new XElement(
                RelPkg + "Relationship",
                new XAttribute("Id", "rId1"),
                new XAttribute(
                    "Type",
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument"),
                new XAttribute("Target", "word/document.xml")),
            new XElement(
                RelPkg + "Relationship",
                new XAttribute("Id", "rId2"),
                new XAttribute(
                    "Type",
                    "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties"),
                new XAttribute("Target", "docProps/core.xml")));
        return new XDocument(root).ToString(SaveOptions.DisableFormatting);
    }

    private static string BuildContentTypes(int count)
    {
        var root = new XElement(
            Ct + "Types",
            new XAttribute("xmlns", Ct.NamespaceName),
            new XElement(
                Ct + "Default",
                new XAttribute("Extension", "rels"),
                new XAttribute("ContentType", "application/vnd.openxmlformats-package.relationships+xml")),
            new XElement(
                Ct + "Default",
                new XAttribute("Extension", "xml"),
                new XAttribute("ContentType", "application/xml")),
            new XElement(
                Ct + "Default",
                new XAttribute("Extension", "bin"),
                new XAttribute(
                    "ContentType",
                    "application/vnd.openxmlformats-officedocument.oleObject")),
            new XElement(
                Ct + "Default",
                new XAttribute("Extension", "wmf"),
                new XAttribute("ContentType", "image/x-wmf")),
            new XElement(
                Ct + "Override",
                new XAttribute("PartName", "/word/document.xml"),
                new XAttribute(
                    "ContentType",
                    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml")),
            new XElement(
                Ct + "Override",
                new XAttribute("PartName", "/docProps/core.xml"),
                new XAttribute(
                    "ContentType",
                    "application/vnd.openxmlformats-package.core-properties+xml")));

        // Keep count referenced so callers know package size; Overrides for bin/wmf use Defaults.
        _ = count;
        return new XDocument(root).ToString(SaveOptions.DisableFormatting);
    }

    private static string BuildCoreProps()
    {
        var now = DateTime.UtcNow.ToString("yyyy-MM-ddTHH:mm:ssZ", CultureInfo.InvariantCulture);
        var root = new XElement(
            Cp + "coreProperties",
            new XAttribute(XNamespace.Xmlns + "cp", Cp),
            new XAttribute(XNamespace.Xmlns + "dc", Dc),
            new XAttribute(XNamespace.Xmlns + "dcterms", DcTerms),
            new XAttribute(XNamespace.Xmlns + "xsi", Xsi),
            new XElement(Dc + "title", "mathset MathType OLE smoke test"),
            new XElement(Dc + "creator", "MathTypeOle.Cli"),
            new XElement(
                DcTerms + "created",
                new XAttribute(Xsi + "type", "dcterms:W3CDTF"),
                now),
            new XElement(
                DcTerms + "modified",
                new XAttribute(Xsi + "type", "dcterms:W3CDTF"),
                now));
        return new XDocument(root).ToString(SaveOptions.DisableFormatting);
    }

    /// <summary>
    /// Read placeable WMF bounding box; fall back to a readable default.
    /// </summary>
    internal static (double widthPt, double heightPt) EstimateWmfSizePt(byte[] wmf)
    {
        const double fallbackW = 36;
        const double fallbackH = 18;
        if (wmf is null || wmf.Length < 22) return (fallbackW, fallbackH);
        // Placeable key 0x9AC6CDD7 little-endian: D7 CD C6 9A
        if (!(wmf[0] == 0xD7 && wmf[1] == 0xCD && wmf[2] == 0xC6 && wmf[3] == 0x9A))
            return (fallbackW, fallbackH);

        var left = BitConverter.ToInt16(wmf, 6);
        var top = BitConverter.ToInt16(wmf, 8);
        var right = BitConverter.ToInt16(wmf, 10);
        var bottom = BitConverter.ToInt16(wmf, 12);
        var inch = BitConverter.ToUInt16(wmf, 14);
        if (inch == 0) inch = 1440;

        var widthIn = Math.Abs(right - left) / (double)inch;
        var heightIn = Math.Abs(bottom - top) / (double)inch;
        var widthPt = widthIn * 72.0;
        var heightPt = heightIn * 72.0;
        if (widthPt < 1 || heightPt < 1) return (fallbackW, fallbackH);
        // Clamp runaway sizes from odd metafiles.
        widthPt = Math.Min(widthPt, 468);
        heightPt = Math.Min(heightPt, 300);
        return (widthPt, heightPt);
    }

    /// <summary>
    /// Reads baseline offset from MathType MFCOMMENT record inside WMF if present;
    /// returns 0 if not found. Value is in typographic points.
    /// </summary>
    internal static double EstimateWmfBaselinePt(byte[] wmf)
    {
        if (wmf is null || wmf.Length < 22) return 0.0;
        // Search for "MathType\0\0" (0x4D 0x61 0x74 0x68 0x54 0x79 0x70 0x65 0x00 0x00)
        var magic = new byte[] { 0x4D, 0x61, 0x74, 0x68, 0x54, 0x79, 0x70, 0x65, 0x00, 0x00 };
        var idx = IndexOf(wmf, magic);
        if (idx != -1 && idx + 12 <= wmf.Length)
        {
            var rawVal = BitConverter.ToInt16(wmf, idx + 10);
            return Math.Round(rawVal / 16.0, 4);
        }
        return 0.0;
    }

    private static int IndexOf(byte[] source, byte[] pattern)
    {
        if (source is null || pattern is null || source.Length < pattern.Length)
            return -1;
        for (var i = 0; i <= source.Length - pattern.Length; i++)
        {
            var match = true;
            for (var j = 0; j < pattern.Length; j++)
            {
                if (source[i + j] != pattern[j])
                {
                    match = false;
                    break;
                }
            }
            if (match) return i;
        }
        return -1;
    }

    private static void WriteEntry(ZipArchive zip, string path, string xml)
    {
        var entry = zip.CreateEntry(path, CompressionLevel.Optimal);
        using var stream = entry.Open();
        using var writer = new StreamWriter(stream, new UTF8Encoding(encoderShouldEmitUTF8Identifier: false));
        writer.Write(xml);
    }

    private static void WriteBinary(ZipArchive zip, string path, byte[] data)
    {
        var entry = zip.CreateEntry(path, CompressionLevel.Optimal);
        using var stream = entry.Open();
        stream.Write(data, 0, data.Length);
    }
}
