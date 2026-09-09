using Microsoft.Win32;

namespace VisualTeX.WordVsto;

/// <summary>
/// MathType OLE ProgID / CLSID identity used when writing standalone CFB storages.
/// Extracted from VisualTeX MathTypeOleInterop (identity subset only).
/// </summary>
internal static class MathTypeOleIdentity
{
    internal const string CanonicalProgId = "Equation.DSMT4";
    internal const string CanonicalUserType = "MathType 7.0 Equation";
    internal static readonly Guid CanonicalClsid =
        new("0002CE03-0000-0000-C000-000000000046");

    internal sealed class StorageIdentity
    {
        public string ProgId { get; set; } = CanonicalProgId;
        public Guid Clsid { get; set; } = CanonicalClsid;
        public string UserType { get; set; } = CanonicalUserType;
    }

    /// <summary>
    /// Prefer a locally registered MathType class when present; otherwise fall back
    /// to the canonical Equation.DSMT4 identity (bin creation does not require MathType).
    /// </summary>
    internal static StorageIdentity ResolvePreferredStorageIdentity()
    {
        foreach (var progId in new[] { CanonicalProgId, "DSEquations", "Equation" })
        {
            if (!TryResolveClsid(progId, out var clsid)) continue;
            if (!LooksLikeMathTypeClass(clsid, out var className)) continue;
            return new StorageIdentity
            {
                ProgId = progId,
                Clsid = clsid,
                UserType = string.IsNullOrWhiteSpace(className)
                    ? CanonicalUserType
                    : className!,
            };
        }

        return new StorageIdentity();
    }

    private static bool TryResolveClsid(string progId, out Guid clsid)
    {
        clsid = Guid.Empty;
        try
        {
            using var key = Registry.ClassesRoot.OpenSubKey(progId + "\\CLSID");
            var text = key?.GetValue(null) as string;
            return !string.IsNullOrWhiteSpace(text) && Guid.TryParse(text, out clsid);
        }
        catch
        {
            return false;
        }
    }

    private static bool LooksLikeMathTypeClass(Guid clsid, out string? className)
    {
        className = null;
        if (clsid == Guid.Empty) return false;
        if (clsid == CanonicalClsid)
        {
            className = CanonicalUserType;
            return true;
        }

        try
        {
            using var classKey = Registry.ClassesRoot.OpenSubKey(
                "CLSID\\{" + clsid.ToString("D") + "}");
            if (classKey is null) return false;
            className = classKey.GetValue(null) as string;
            var server = classKey.OpenSubKey("LocalServer32")?.GetValue(null) as string
                ?? classKey.OpenSubKey("LocalServer")?.GetValue(null) as string;
            var fileName = string.IsNullOrWhiteSpace(server)
                ? null
                : Path.GetFileName(server!.Trim().Trim('"'));
            return (!string.IsNullOrWhiteSpace(className)
                    && className!.IndexOf("MathType", StringComparison.OrdinalIgnoreCase) >= 0)
                || string.Equals(fileName, "MathType.exe", StringComparison.OrdinalIgnoreCase);
        }
        catch
        {
            return false;
        }
    }
}
