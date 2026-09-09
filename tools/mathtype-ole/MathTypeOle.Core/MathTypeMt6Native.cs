using System.Runtime.InteropServices;

namespace VisualTeX.WordVsto;

/// <summary>
/// P/Invoke surface for MathType MT6.dll (SDK transform APIs).
/// Constants match Design Science / Wiris MathType SDK (MTSDKDN).
/// </summary>
internal static class MathTypeMt6Native
{
    internal const short MtOk = 0;
    internal const short MtNotFound = -1;
    internal const short MtCantRun = -2;
    internal const short MtBadVersion = -3;
    internal const short MtInUse = -4;
    internal const short MtNotRunning = -5;
    internal const short MtRunTimeout = -6;
    internal const short MtNotEquation = -7;
    internal const short MtFileNotFound = -8;
    internal const short MtMemory = -9;
    internal const short MtTranslatorError = -14;
    internal const short MtPreferenceError = -15;
    internal const short MtBadPath = -16;
    internal const short MtError = -9999;

    internal const short MtInitLaunchAsNeeded = 0;
    internal const short MtInitLaunchNow = 1;

    internal const short MtxfmPrevious = -1;
    internal const short MtxfmClipboard = -2;
    internal const short MtxfmLocal = -3;
    internal const short MtxfmFile = -4;

    internal const short MtxfmMtef = 4;
    internal const short MtxfmHmtef = 5;
    internal const short MtxfmPict = 6;
    internal const short MtxfmText = 7;
    internal const short MtxfmHtext = 8;
    internal const short MtxfmGif = 9;

    internal const short MtxfmPrefExisting = 1;
    internal const short MtxfmPrefMtDefault = 2;
    internal const short MtxfmPrefUser = 3;

    internal const short MtxfmStatActualLen = -1;
    internal const short MtxfmStatTransl = -2;
    internal const short MtxfmStatPref = -3;

    private static readonly object Gate = new();
    private static bool _directoryPrepared;
    private static IntPtr _module;

    internal static string ResolveMt6Directory()
    {
        var overrideDir = Environment.GetEnvironmentVariable("MATHTYPE_MT6_DIR");
        if (!string.IsNullOrWhiteSpace(overrideDir) && Directory.Exists(overrideDir))
            return Path.GetFullPath(overrideDir);

        var overrideDll = Environment.GetEnvironmentVariable("MATHTYPE_MT6_DLL");
        if (!string.IsNullOrWhiteSpace(overrideDll) && File.Exists(overrideDll))
            return Path.GetDirectoryName(Path.GetFullPath(overrideDll))!;

        // Prefer architecture-matched System\{32|64}\ copy.
        var root = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.ProgramFilesX86),
            "MathType",
            "System");
        var arch = Environment.Is64BitProcess ? "64" : "32";
        var archDir = Path.Combine(root, arch);
        if (File.Exists(Path.Combine(archDir, "MT6.dll")))
            return archDir;

        if (File.Exists(Path.Combine(root, "MT6.dll")))
            return root;

        throw new FileNotFoundException(
            "MT6.dll not found. Install MathType or set MATHTYPE_MT6_DIR / MATHTYPE_MT6_DLL. "
            + $"Looked under '{root}'.");
    }

    internal static void EnsureLoaded()
    {
        lock (Gate)
        {
            if (_module != IntPtr.Zero) return;
            var dir = ResolveMt6Directory();
            var dllPath = Path.Combine(dir, "MT6.dll");
            if (!File.Exists(dllPath))
                throw new FileNotFoundException($"MT6.dll missing at '{dllPath}'.");

            if (!_directoryPrepared)
            {
                // Help dependent MathType binaries resolve next to MT6.dll.
                SetDllDirectory(dir);
                _directoryPrepared = true;
            }

            _module = LoadLibrary(dllPath);
            if (_module == IntPtr.Zero)
            {
                var err = Marshal.GetLastWin32Error();
                throw new DllNotFoundException(
                    $"LoadLibrary failed for '{dllPath}' (Win32={err}). "
                    + "Ensure the process bitness matches the DLL (x64 ↔ System\\64).");
            }
        }
    }

    internal static string DescribeStatus(int code) => code switch
    {
        MtOk => "mtOK",
        MtNotFound => "mtNOT_FOUND (MathType server / session)",
        MtCantRun => "mtCANT_RUN (could not start MathType)",
        MtBadVersion => "mtBAD_VERSION",
        MtInUse => "mtIN_USE",
        MtNotRunning => "mtNOT_RUNNING",
        MtRunTimeout => "mtRUN_TIMEOUT",
        MtNotEquation => "mtNOT_EQUATION",
        MtFileNotFound => "mtFILE_NOT_FOUND",
        MtMemory => "mtMEMORY",
        MtTranslatorError => "mtTRANSLATOR_ERROR",
        MtPreferenceError => "mtPREFERENCE_ERROR",
        MtBadPath => "mtBAD_PATH",
        MtError => "mtERROR",
        _ => $"status={code}",
    };

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool SetDllDirectory(string lpPathName);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadLibrary(string lpFileName);

    [DllImport("MT6.dll", CallingConvention = CallingConvention.Winapi)]
    internal static extern int MTAPIConnect(short mtStart, short timeout);

    [DllImport("MT6.dll", CallingConvention = CallingConvention.Winapi)]
    internal static extern int MTAPIDisconnect();

    [DllImport("MT6.dll", CallingConvention = CallingConvention.Winapi)]
    internal static extern int MTXFormReset();

    [DllImport("MT6.dll", CallingConvention = CallingConvention.Winapi, CharSet = CharSet.Ansi)]
    internal static extern int MTXFormSetPrefs(short prefType, [MarshalAs(UnmanagedType.LPStr)] string? prefStr);

    [DllImport("MT6.dll", CallingConvention = CallingConvention.Winapi)]
    internal static extern int MTXFormGetStatus(short index);

    [DllImport("MT6.dll", CallingConvention = CallingConvention.Winapi)]
    internal static extern int MTXFormEqn(
        short src,
        short srcFmt,
        byte[]? srcData,
        int srcDataLen,
        short dst,
        short dstFmt,
        IntPtr dstData,
        int dstDataLen,
        [MarshalAs(UnmanagedType.LPStr)] string? dstPath,
        IntPtr dims);

    [DllImport("MT6.dll", CallingConvention = CallingConvention.Winapi)]
    internal static extern int MTGetLastDimension(short dimIndex);
}
