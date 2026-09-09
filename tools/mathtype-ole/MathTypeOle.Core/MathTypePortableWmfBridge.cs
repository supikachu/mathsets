using System.Diagnostics;
using System.Globalization;
using System.Text.Json;
using System.Text.Json.Serialization;

namespace VisualTeX.WordVsto;

/// <summary>
/// Invokes <c>tools/mt_converter_portable/mt_converter.exe</c> (bin/MTEF → WMF + baseline).
/// Prefers the portable core next to that exe over a system MathType install.
/// </summary>
public static class MathTypePortableWmfBridge
{
    public sealed class Result
    {
        public string WmfPath { get; set; } = string.Empty;
        public double BaselineOffsetPt { get; set; }
        public double WidthPt { get; set; }
        public double HeightPt { get; set; }
        public string Method { get; set; } = string.Empty;
        public int Code { get; set; }
    }

    private sealed class JsonPayload
    {
        [JsonPropertyName("output")] public string? Output { get; set; }
        [JsonPropertyName("baseline_offset_pt")] public double BaselineOffsetPt { get; set; }
        [JsonPropertyName("width_pt")] public double WidthPt { get; set; }
        [JsonPropertyName("height_pt")] public double HeightPt { get; set; }
        [JsonPropertyName("method")] public string? Method { get; set; }
        [JsonPropertyName("code")] public int Code { get; set; }
    }

    /// <summary>
    /// True when mt_converter.exe (+ mathtype_core) is discoverable.
    /// </summary>
    public static bool IsAvailable() =>
        TryResolveConverterExe(out _) && TryResolveCoreDir(out _);

    public static bool TryConvertFile(string inputPath, string wmfPath, out Result result)
    {
        result = new Result();
        if (!TryResolveConverterExe(out var exe)) return false;
        if (!TryResolveCoreDir(out var coreDir)) return false;
        if (string.IsNullOrWhiteSpace(inputPath) || !File.Exists(inputPath)) return false;
        if (string.IsNullOrWhiteSpace(wmfPath)) return false;

        var fullWmf = Path.GetFullPath(wmfPath);
        var dir = Path.GetDirectoryName(fullWmf);
        if (!string.IsNullOrEmpty(dir))
            Directory.CreateDirectory(dir);

        var psi = new ProcessStartInfo
        {
            FileName = exe,
            Arguments = string.Join(
                " ",
                "-i",
                Quote(Path.GetFullPath(inputPath)),
                "-o",
                Quote(fullWmf),
                "--core-dir",
                Quote(coreDir),
                "--json"),
            UseShellExecute = false,
            CreateNoWindow = true,
            RedirectStandardOutput = true,
            RedirectStandardError = true,
            WorkingDirectory = Path.GetDirectoryName(exe) ?? Environment.CurrentDirectory,
        };

        using var process = Process.Start(psi);
        if (process is null) return false;
        var stdout = process.StandardOutput.ReadToEnd();
        var stderr = process.StandardError.ReadToEnd();
        if (!process.WaitForExit(120_000))
        {
            try { process.Kill(); } catch { }
            return false;
        }

        if (process.ExitCode != 0)
            return false;

        try
        {
            var json = JsonSerializer.Deserialize<JsonPayload>(stdout.Trim());
            if (json is null || json.Code != 0) return false;
            if (!File.Exists(fullWmf) || new FileInfo(fullWmf).Length < 22) return false;

            result = new Result
            {
                WmfPath = fullWmf,
                BaselineOffsetPt = json.BaselineOffsetPt,
                WidthPt = json.WidthPt,
                HeightPt = json.HeightPt,
                Method = json.Method ?? "portable",
                Code = json.Code,
            };
            return true;
        }
        catch
        {
            if (!string.IsNullOrWhiteSpace(stderr))
                Debug.WriteLine(stderr);
            return File.Exists(fullWmf) && new FileInfo(fullWmf).Length >= 22;
        }
    }

    public static bool TryConvertBytes(byte[] payload, string wmfPath, out Result result)
    {
        result = new Result();
        var tempRoot = Path.Combine(Path.GetTempPath(), "mathset-mathtype-ole");
        Directory.CreateDirectory(tempRoot);
        var tempIn = Path.Combine(tempRoot, $"portable-in-{Guid.NewGuid():N}.bin");
        try
        {
            File.WriteAllBytes(tempIn, payload);
            return TryConvertFile(tempIn, wmfPath, out result);
        }
        finally
        {
            try { if (File.Exists(tempIn)) File.Delete(tempIn); }
            catch { }
        }
    }

    public static bool TryResolveConverterExe(out string exePath)
    {
        exePath = string.Empty;
        var env = Environment.GetEnvironmentVariable("MATHTYPE_PORTABLE_CONVERTER");
        if (!string.IsNullOrWhiteSpace(env) && File.Exists(env))
        {
            exePath = Path.GetFullPath(env);
            return true;
        }

        foreach (var candidate in CandidateConverterPaths())
        {
            if (File.Exists(candidate))
            {
                exePath = Path.GetFullPath(candidate);
                return true;
            }
        }

        return false;
    }

    public static bool TryResolveCoreDir(out string coreDir)
    {
        coreDir = string.Empty;
        var env = Environment.GetEnvironmentVariable("MATHTYPE_PORTABLE_CORE");
        if (!string.IsNullOrWhiteSpace(env)
            && File.Exists(Path.Combine(env, "MT6.dll")))
        {
            coreDir = Path.GetFullPath(env);
            return true;
        }

        if (TryResolveConverterExe(out var exe))
        {
            var beside = Path.Combine(Path.GetDirectoryName(exe)!, "mathtype_core");
            if (File.Exists(Path.Combine(beside, "MT6.dll")))
            {
                coreDir = Path.GetFullPath(beside);
                return true;
            }
        }

        foreach (var candidate in CandidateCorePaths())
        {
            if (File.Exists(Path.Combine(candidate, "MT6.dll")))
            {
                coreDir = Path.GetFullPath(candidate);
                return true;
            }
        }

        return false;
    }

    private static string Quote(string path) =>
        "\"" + path.Replace("\"", "\\\"") + "\"";

    private static IEnumerable<string> CandidateConverterPaths()
    {
        yield return Path.Combine(
            Environment.CurrentDirectory,
            "mt_converter_portable",
            "mt_converter.exe");
        yield return Path.Combine(
            Environment.CurrentDirectory,
            "..",
            "mt_converter_portable",
            "mt_converter.exe");
        yield return Path.GetFullPath(Path.Combine(
            AppContext.BaseDirectory,
            "..",
            "..",
            "..",
            "..",
            "..",
            "mt_converter_portable",
            "mt_converter.exe"));
        yield return Path.GetFullPath(Path.Combine(
            AppContext.BaseDirectory,
            "..",
            "..",
            "..",
            "..",
            "mt_converter_portable",
            "mt_converter.exe"));

        // tools/mathtype-ole/... → tools/mt_converter_portable
        var dir = new DirectoryInfo(AppContext.BaseDirectory);
        for (var i = 0; i < 10 && dir is not null; i++, dir = dir.Parent)
        {
            if (!string.Equals(dir.Name, "tools", StringComparison.OrdinalIgnoreCase)
                && !string.Equals(dir.Name, "mathset", StringComparison.OrdinalIgnoreCase))
                continue;
            var underTools = Path.Combine(dir.FullName, "mt_converter_portable", "mt_converter.exe");
            if (dir.Name.Equals("tools", StringComparison.OrdinalIgnoreCase))
                yield return underTools;
            if (dir.Name.Equals("mathset", StringComparison.OrdinalIgnoreCase))
                yield return Path.Combine(dir.FullName, "tools", "mt_converter_portable", "mt_converter.exe");
        }
    }

    private static IEnumerable<string> CandidateCorePaths()
    {
        foreach (var exe in CandidateConverterPaths())
        {
            var parent = Path.GetDirectoryName(exe);
            if (!string.IsNullOrEmpty(parent))
                yield return Path.Combine(parent, "mathtype_core");
        }
    }
}
