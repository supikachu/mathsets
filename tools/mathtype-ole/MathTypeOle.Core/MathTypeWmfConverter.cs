namespace VisualTeX.WordVsto;

/// <summary>
/// MathML / MTEF / Equation.Native / ole.bin → authentic MathType WMF.
/// Prefers <see cref="MathTypePortableWmfBridge"/> when
/// <c>tools/mt_converter_portable</c> is present; otherwise falls back to
/// in-process <c>MT6.dll</c> (system MathType install).
/// </summary>
public static class MathTypeWmfConverter
{
    /// <summary>
    /// When true (default), try the portable converter before in-process MT6.
    /// Set env <c>MATHTYPE_WMF_ENGINE=mt6</c> to force the in-process path.
    /// </summary>
    public static bool PreferPortableConverter
    {
        get
        {
            var engine = Environment.GetEnvironmentVariable("MATHTYPE_WMF_ENGINE");
            if (string.Equals(engine, "mt6", StringComparison.OrdinalIgnoreCase)
                || string.Equals(engine, "inline", StringComparison.OrdinalIgnoreCase))
                return false;
            if (string.Equals(engine, "portable", StringComparison.OrdinalIgnoreCase))
                return true;
            return true;
        }
    }

    /// <summary>
    /// MathML → WMF using our MTEF codec, then MathType render (portable or MT6).
    /// </summary>
    public static void WriteWmfFromMathMl(string mathMl, string wmfPath, bool inline = false)
    {
        if (string.IsNullOrWhiteSpace(mathMl))
            throw new ArgumentException("MathML is empty.", nameof(mathMl));
        var native = MathTypeMtefCodec.CreateEquationNative(mathMl, inline);
        WriteWmfFromMtef(native.Mtef, wmfPath);
    }

    public static void WriteWmfFromMathMl(
        string mathMl,
        string wmfPath,
        bool inline,
        double fontSizePt)
    {
        if (string.IsNullOrWhiteSpace(mathMl))
            throw new ArgumentException("MathML is empty.", nameof(mathMl));
        var native = MathTypeMtefCodec.CreateEquationNativeAtFontSize(mathMl, inline, fontSizePt);
        WriteWmfFromMtef(native.Mtef, wmfPath);
    }

    /// <summary>
    /// Raw MTEF v5 bytes (no 28-byte OLE header) → WMF.
    /// </summary>
    public static void WriteWmfFromMtef(byte[] mtef, string wmfPath)
    {
        if (mtef is null || mtef.Length < 12)
            throw new ArgumentException("MTEF payload is too short.", nameof(mtef));
        if (mtef[0] != 5)
            throw new InvalidDataException($"Expected MTEF v5; actual version byte={mtef[0]}.");
        if (string.IsNullOrWhiteSpace(wmfPath))
            throw new ArgumentException("WMF output path is required.", nameof(wmfPath));

        var fullPath = Path.GetFullPath(wmfPath);
        var dir = Path.GetDirectoryName(fullPath);
        if (!string.IsNullOrEmpty(dir))
            Directory.CreateDirectory(dir);
        if (File.Exists(fullPath))
            File.Delete(fullPath);

        if (PreferPortableConverter
            && MathTypePortableWmfBridge.TryConvertBytes(mtef, fullPath, out _))
            return;

        WriteWmfFromMtefInlineMt6(mtef, fullPath);
    }

    /// <summary>
    /// Equation Native stream (28-byte OLE header + MTEF) → WMF.
    /// </summary>
    public static void WriteWmfFromEquationNative(byte[] equationNative, string wmfPath)
    {
        var mtef = MathTypeMtefCodec.GetMtefFromEquationNative(equationNative);
        WriteWmfFromMtef(mtef, wmfPath);
    }

    /// <summary>
    /// Standalone MathType ole.bin (CFB) → WMF.
    /// </summary>
    public static void WriteWmfFromOleBin(byte[] compoundFile, string wmfPath)
    {
        if (PreferPortableConverter
            && MathTypePortableWmfBridge.TryConvertBytes(compoundFile, wmfPath, out _))
            return;

        var equationNative = MathTypeOleStorage.ReadEquationNative(compoundFile);
        WriteWmfFromEquationNative(equationNative, wmfPath);
    }

    public static void WriteWmfFromOleBinFile(string oleBinPath, string wmfPath)
    {
        if (PreferPortableConverter
            && MathTypePortableWmfBridge.TryConvertFile(oleBinPath, wmfPath, out _))
            return;

        var bytes = File.ReadAllBytes(oleBinPath);
        var equationNative = MathTypeOleStorage.ReadEquationNative(bytes);
        var mtef = MathTypeMtefCodec.GetMtefFromEquationNative(equationNative);
        WriteWmfFromMtefInlineMt6(mtef, Path.GetFullPath(wmfPath));
    }

    /// <summary>
    /// True if the file looks like a placeable or standard WMF (heuristic).
    /// </summary>
    public static bool LooksLikeWmf(byte[] data)
    {
        if (data is null || data.Length < 22) return false;
        if (data[0] == 0xD7 && data[1] == 0xCD && data[2] == 0xC6 && data[3] == 0x9A)
            return true;
        var type = BitConverter.ToUInt16(data, 0);
        var headerSize = BitConverter.ToUInt16(data, 2);
        return type is 1 or 2 && headerSize >= 9;
    }

    private static void WriteWmfFromMtefInlineMt6(byte[] mtef, string fullPath)
    {
        var dir = Path.GetDirectoryName(fullPath);
        if (!string.IsNullOrEmpty(dir))
            Directory.CreateDirectory(dir);
        if (File.Exists(fullPath))
            File.Delete(fullPath);

        MathTypeMt6Native.EnsureLoaded();

        var connect = MathTypeMt6Native.MTAPIConnect(
            MathTypeMt6Native.MtInitLaunchAsNeeded,
            timeout: 60);
        if (connect != MathTypeMt6Native.MtOk)
            throw new InvalidOperationException(
                "MTAPIConnect failed: " + MathTypeMt6Native.DescribeStatus(connect)
                + ". Tip: install MathType or place tools/mt_converter_portable with mathtype_core.");

        try
        {
            CheckOk(MathTypeMt6Native.MTXFormReset(), "MTXFormReset");
            CheckOk(
                MathTypeMt6Native.MTXFormSetPrefs(MathTypeMt6Native.MtxfmPrefMtDefault, null),
                "MTXFormSetPrefs");

            var rc = MathTypeMt6Native.MTXFormEqn(
                MathTypeMt6Native.MtxfmLocal,
                MathTypeMt6Native.MtxfmMtef,
                mtef,
                mtef.Length,
                MathTypeMt6Native.MtxfmFile,
                MathTypeMt6Native.MtxfmPict,
                IntPtr.Zero,
                0,
                fullPath,
                IntPtr.Zero);

            if (rc != MathTypeMt6Native.MtOk)
            {
                var pref = MathTypeMt6Native.MTXFormGetStatus(MathTypeMt6Native.MtxfmStatPref);
                var transl = MathTypeMt6Native.MTXFormGetStatus(MathTypeMt6Native.MtxfmStatTransl);
                throw new InvalidOperationException(
                    "MTXFormEqn(MTEF→PICT/file) failed: "
                    + MathTypeMt6Native.DescribeStatus(rc)
                    + $"; pref={MathTypeMt6Native.DescribeStatus(pref)}"
                    + $"; transl={MathTypeMt6Native.DescribeStatus(transl)}");
            }

            if (!File.Exists(fullPath) || new FileInfo(fullPath).Length < 22)
                throw new InvalidDataException(
                    $"MathType did not produce a usable WMF at '{fullPath}'.");
        }
        finally
        {
            try { MathTypeMt6Native.MTAPIDisconnect(); }
            catch { /* ignore disconnect failures */ }
        }
    }

    private static void CheckOk(int code, string api)
    {
        if (code != MathTypeMt6Native.MtOk)
            throw new InvalidOperationException(
                $"{api} failed: {MathTypeMt6Native.DescribeStatus(code)}");
    }
}
