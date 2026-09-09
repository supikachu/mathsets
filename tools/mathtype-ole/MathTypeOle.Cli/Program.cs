using VisualTeX.WordVsto;

namespace MathTypeOle.Cli;

internal static class Program
{
    private static int Main(string[] args)
    {
        try
        {
            return Run(args);
        }
        catch (Exception ex)
        {
            Console.Error.WriteLine(ex.Message);
            return 1;
        }
    }

    private static int Run(string[] args)
    {
        if (args.Length == 0 || HasFlag(args, "-h") || HasFlag(args, "--help"))
        {
            PrintUsage();
            return args.Length == 0 ? 1 : 0;
        }

        var inline = HasFlag(args, "--inline");
        double? fontSizePt = ParseFontSize(args);

        if (TryGetOption(args, "--from-ole", out var fromOle))
        {
            var outPath = ResolveOutput(args, fallbackFrom: fromOle, defaultExt: ".wmf");
            EnsureParentDir(outPath);
            MathTypeWmfConverter.WriteWmfFromOleBinFile(fromOle, outPath);
            ReportWmf(args, outPath);
            return 0;
        }

        if (TryGetOption(args, "--from-mtef", out var fromMtef))
        {
            var outPath = ResolveOutput(args, fallbackFrom: fromMtef, defaultExt: ".wmf");
            EnsureParentDir(outPath);
            MathTypeWmfConverter.WriteWmfFromMtef(File.ReadAllBytes(fromMtef), outPath);
            ReportWmf(args, outPath);
            return 0;
        }

        if (HasFlag(args, "--demo-docx"))
        {
            var outPath = ResolveOutput(args, fallbackFrom: "mathtype-demo.docx", defaultExt: ".docx");
            if (!outPath.EndsWith(".docx", StringComparison.OrdinalIgnoreCase))
                outPath = Path.ChangeExtension(outPath, ".docx");
            WriteDemoDocx(args, outPath, fontSizePt);
            return 0;
        }

        if (TryGetOption(args, "--docx", out var docxPath))
        {
            WriteDemoDocx(args, docxPath, fontSizePt);
            return 0;
        }

        var positional = FilterPositionals(args);
        string mathMl;
        string? explicitOutput = null;
        TryGetOption(args, "-o", out explicitOutput);
        if (string.IsNullOrEmpty(explicitOutput))
            TryGetOption(args, "--output", out explicitOutput);

        if (HasFlag(args, "--stdin") || (positional.Length >= 1 && positional[0] == "-"))
        {
            mathMl = Console.In.ReadToEnd();
            if (string.IsNullOrWhiteSpace(explicitOutput) && positional.Length >= 2)
                explicitOutput = positional[positional.Length - 1];
            if (string.IsNullOrWhiteSpace(explicitOutput))
                throw new ArgumentException("When reading stdin, pass -o/--output.");
        }
        else
        {
            if (positional.Length < 1)
                throw new ArgumentException(
                    "Missing MathML input path (or use --stdin / --from-ole / --from-mtef).");
            mathMl = File.ReadAllText(positional[0]);
            if (string.IsNullOrWhiteSpace(explicitOutput) && positional.Length >= 2)
                explicitOutput = positional[1];
            if (string.IsNullOrWhiteSpace(explicitOutput))
            {
                var flagWmf = HasFlag(args, "--wmf") && !HasFlag(args, "--bin");
                explicitOutput = Path.ChangeExtension(
                    positional[0],
                    flagWmf ? ".wmf" : ".bin");
            }
        }

        if (string.IsNullOrWhiteSpace(mathMl))
            throw new InvalidDataException("MathML input is empty.");

        var (wantBin, wantWmf) = ResolveModes(args, explicitOutput!);
        string? binPath = null;
        string? wmfPath = null;
        var ext = Path.GetExtension(explicitOutput!);

        if (wantBin && wantWmf)
        {
            if (ext.Equals(".wmf", StringComparison.OrdinalIgnoreCase))
            {
                wmfPath = explicitOutput;
                binPath = Path.ChangeExtension(explicitOutput, ".bin");
            }
            else
            {
                binPath = explicitOutput;
                wmfPath = Path.ChangeExtension(explicitOutput, ".wmf");
            }
        }
        else if (wantWmf)
        {
            wmfPath = ext.Equals(".wmf", StringComparison.OrdinalIgnoreCase)
                ? explicitOutput
                : Path.ChangeExtension(explicitOutput, ".wmf");
        }
        else
        {
            binPath = explicitOutput;
        }

        if (wantBin)
        {
            var bytes = fontSizePt.HasValue
                ? MathTypeOleStorage.CreateStandaloneCompoundFile(mathMl, inline, fontSizePt.Value)
                : MathTypeOleStorage.CreateStandaloneCompoundFile(mathMl, inline);
            EnsureParentDir(binPath!);
            File.WriteAllBytes(binPath!, bytes);
            if (!HasFlag(args, "--quiet"))
            {
                Console.WriteLine($"Wrote {bytes.Length} bytes → {binPath}");
                if (HasFlag(args, "--verify"))
                {
                    var roundTrip = MathTypeOleStorage.ReadMathMl(bytes);
                    Console.WriteLine($"Verify MathML length: {roundTrip.Length} chars");
                }
            }
        }

        if (wantWmf)
        {
            EnsureParentDir(wmfPath!);
            if (fontSizePt.HasValue)
                MathTypeWmfConverter.WriteWmfFromMathMl(mathMl, wmfPath!, inline, fontSizePt.Value);
            else
                MathTypeWmfConverter.WriteWmfFromMathMl(mathMl, wmfPath!, inline);
            ReportWmf(args, wmfPath!);
        }

        return 0;
    }

    private static (bool wantBin, bool wantWmf) ResolveModes(string[] args, string outputPath)
    {
        var hasBin = HasFlag(args, "--bin");
        var hasWmf = HasFlag(args, "--wmf");
        if (hasBin && hasWmf) return (true, true);
        if (hasWmf) return (false, true);
        if (hasBin) return (true, false);

        var ext = Path.GetExtension(outputPath);
        if (ext.Equals(".wmf", StringComparison.OrdinalIgnoreCase))
            return (false, true);
        return (true, false);
    }

    private static double? ParseFontSize(string[] args)
    {
        if (!TryGetOption(args, "--font-size", out var fontText))
            return null;
        if (!double.TryParse(
                fontText,
                System.Globalization.NumberStyles.Float,
                System.Globalization.CultureInfo.InvariantCulture,
                out var pt)
            || pt <= 0)
            throw new ArgumentException("--font-size must be a positive number (points).");
        return pt;
    }

    private static string ResolveOutput(string[] args, string fallbackFrom, string defaultExt)
    {
        if (TryGetOption(args, "-o", out var o) || TryGetOption(args, "--output", out o))
            return o;
        var positional = FilterPositionals(args);
        if (positional.Length >= 1)
            return positional[0];
        return Path.ChangeExtension(fallbackFrom, defaultExt);
    }

    private static string[] FilterPositionals(string[] args)
    {
        var skipNext = false;
        var optionsWithValue = new HashSet<string>(StringComparer.OrdinalIgnoreCase)
        {
            "-o", "--output", "--font-size", "--from-ole", "--from-mtef", "--docx",
        };
        var list = new List<string>();
        foreach (var a in args)
        {
            if (skipNext)
            {
                skipNext = false;
                continue;
            }

            if (optionsWithValue.Contains(a))
            {
                skipNext = true;
                continue;
            }

            if (a.StartsWith("-", StringComparison.Ordinal))
                continue;
            list.Add(a);
        }

        return list.ToArray();
    }

    private static void WriteDemoDocx(string[] args, string docxPath, double? fontSizePt)
    {
        EnsureParentDir(docxPath);
        var equations = LoadDemoEquations();
        MathTypeDocxWriter.WriteDemoDocxFromMathMl(equations, docxPath, fontSizePt);
        if (!HasFlag(args, "--quiet"))
        {
            var info = new FileInfo(docxPath);
            Console.WriteLine(
                $"Wrote demo DOCX with {equations.Count} MathType equations → {docxPath} ({info.Length} bytes)");
            Console.WriteLine("Open in Word and double-click a formula to verify MathType.");
        }
    }

    private static List<(string caption, string mathMl, bool inline)> LoadDemoEquations()
    {
        var samplesDir = FindSamplesDir();
        var list = new List<(string, string, bool)>
        {
            ("Fraction a/b (display)", ReadSample(samplesDir, "01-frac.mathml.xml"), false),
            ("Sqrt + superscript", ReadSample(samplesDir, "02-sqrt-sup.mathml.xml"), false),
            ("Stretchy braces", ReadSample(samplesDir, "03-fence.mathml.xml"), false),
            ("Inline E=mc^2", ReadSample(samplesDir, "04-inline.mathml.xml"), true),
        };
        return list;
    }

    private static string FindSamplesDir()
    {
        var candidates = new[]
        {
            Path.Combine(Environment.CurrentDirectory, "samples"),
            Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "..", "samples")),
            Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "..", "..", "..", "samples")),
            Path.Combine(AppContext.BaseDirectory, "samples"),
        };
        foreach (var c in candidates)
        {
            if (Directory.Exists(c) && File.Exists(Path.Combine(c, "01-frac.mathml.xml")))
                return c;
        }

        throw new DirectoryNotFoundException(
            "Could not find samples/. Run from tools/mathtype-ole or pass files next to the CLI.");
    }

    private static string ReadSample(string samplesDir, string name)
    {
        var path = Path.Combine(samplesDir, name);
        if (!File.Exists(path))
            throw new FileNotFoundException($"Sample missing: {path}");
        return File.ReadAllText(path);
    }

    private static void EnsureParentDir(string path)
    {
        var dir = Path.GetDirectoryName(Path.GetFullPath(path));
        if (!string.IsNullOrEmpty(dir))
            Directory.CreateDirectory(dir);
    }

    private static void ReportWmf(string[] args, string path)
    {
        if (HasFlag(args, "--quiet")) return;
        var data = File.ReadAllBytes(path);
        var kind = MathTypeWmfConverter.LooksLikeWmf(data) ? "WMF" : "unknown-metafile";
        var engine = MathTypePortableWmfBridge.IsAvailable()
            && MathTypeWmfConverter.PreferPortableConverter
            ? "portable"
            : "mt6-inline";
        Console.WriteLine($"Wrote {data.Length} bytes → {path} ({kind}, engine={engine})");
    }

    private static bool HasFlag(string[] args, string name) =>
        args.Any(a => string.Equals(a, name, StringComparison.OrdinalIgnoreCase));

    private static bool TryGetOption(string[] args, string name, out string value)
    {
        for (var i = 0; i < args.Length - 1; i++)
        {
            if (string.Equals(args[i], name, StringComparison.OrdinalIgnoreCase))
            {
                value = args[i + 1];
                return true;
            }
        }

        value = string.Empty;
        return false;
    }

    private static void PrintUsage()
    {
        Console.WriteLine("""
            MathTypeOle.Cli — MathML → ole.bin / MathType WMF / demo DOCX (Windows x64)

            Usage:
              MathTypeOle.Cli <input.mathml.xml> [output.bin] [--verify]
              MathTypeOle.Cli <input.mathml.xml> out.wmf --wmf
              MathTypeOle.Cli <input.mathml.xml> out.bin --bin --wmf
              MathTypeOle.Cli --from-ole equation.bin -o preview.wmf
              MathTypeOle.Cli --from-mtef equation.mtef -o preview.wmf
              MathTypeOle.Cli --demo-docx -o samples\out\mathtype-demo.docx
              MathTypeOle.Cli --docx samples\out\mathtype-demo.docx

            Options:
              --bin          Write MathType ole.bin
              --wmf          Write authentic MathType WMF via MT6.dll
              --demo-docx    Build a Word .docx with sample MathType OLE formulas
              --docx PATH    Same as --demo-docx with explicit output path
              --from-ole P   Read ole.bin / CFB and emit WMF
              --from-mtef P  Read raw MTEF v5 bytes and emit WMF
              --inline       Mark equation as inline (default: display)
              --font-size N  MathType Full size in points
              --verify       Round-trip read MathML from written CFB
              --quiet        Suppress status lines
              -o, --output   Output path
              -h, --help     Show this help

            Environment:
              MATHTYPE_MT6_DIR   Directory containing MT6.dll (default: MathType\System\64)
              MATHTYPE_MT6_DLL   Full path to MT6.dll

            Notes:
              - Build/run as x64.
              - WMF prefers tools/mt_converter_portable when present (baseline-aware).
              - Force engine: MATHTYPE_WMF_ENGINE=portable|mt6
              - ole.bin does not require MathType; WMF needs portable core or system install.
            """);
    }
}
