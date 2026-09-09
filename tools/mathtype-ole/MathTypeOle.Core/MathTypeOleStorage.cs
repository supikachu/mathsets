using System.Runtime.InteropServices;
using System.Runtime.InteropServices.ComTypes;

namespace VisualTeX.WordVsto;

/// <summary>
/// Creates standalone MathType OLE Compound File Binary payloads (ole.bin)
/// without activating MathType. Slimmed from VisualTeX MathTypeOleStorage —
/// create path only (no Word clipboard / InlineShape APIs).
/// </summary>
public static class MathTypeOleStorage
{
    public static readonly Guid MathTypeEquationClsid = MathTypeOleIdentity.CanonicalClsid;

    private const int StgmRead = 0x00000000;
    private const int StgmReadWrite = 0x00000002;
    private const int StgmShareExclusive = 0x00000010;
    private const int StgmCreate = 0x00001000;
    private const int StatFlagNoName = 1;
    public const int MaximumCompoundFileBytes = 64 * 1024 * 1024;

    /// <summary>
    /// MathML → MathType Equation.DSMT4 compound file (ole.bin bytes).
    /// </summary>
    public static byte[] CreateStandaloneCompoundFile(string mathMl, bool inline) =>
        CreateStandaloneCompoundFile(MathTypeMtefCodec.CreateEquationNative(mathMl, inline));

    /// <summary>
    /// MathML → ole.bin with MathType Full font size (points).
    /// </summary>
    public static byte[] CreateStandaloneCompoundFile(
        string mathMl,
        bool inline,
        double fontSizePt) =>
        CreateStandaloneCompoundFile(
            MathTypeMtefCodec.CreateEquationNativeAtFontSize(mathMl, inline, fontSizePt));

    private static byte[] CreateStandaloneCompoundFile(
        MathTypeMtefCodec.RewriteResult equation)
    {
        if (equation is null
            || equation.EquationNative.Length == 0
            || equation.Mtef.Length == 0)
            throw new InvalidDataException(
                "Standalone MathType OLE creation requires generated Equation Native data.");

        var root = Path.Combine(
            Path.GetTempPath(),
            "mathset-mathtype-ole");
        Directory.CreateDirectory(root);
        var path = Path.Combine(root, $"mathtype-create-{Guid.NewGuid():N}.ole");
        IStorageNative? storage = null;
        try
        {
            var result = StgCreateDocfile(
                path,
                StgmCreate | StgmReadWrite | StgmShareExclusive,
                0,
                out storage);
            if (result < 0) Marshal.ThrowExceptionForHR(result);

            var identity = MathTypeOleIdentity.ResolvePreferredStorageIdentity();
            var clsid = identity.Clsid;
            storage.SetClass(ref clsid);

            var mathTypeFormat = RegisterClipboardFormatW("MathType EF");
            if (mathTypeFormat == 0)
                throw new InvalidOperationException(
                    "Could not register the MathType EF clipboard format.");

            result = WriteFmtUserTypeStg(
                storage,
                checked((ushort)mathTypeFormat),
                identity.UserType);
            if (result < 0) Marshal.ThrowExceptionForHR(result);

            // Standard OLE2 containment metadata streams written by MathType 7.
            CreateAndWriteStream(
                storage,
                "\u0001Ole",
                new byte[]
                {
                    0x01, 0x00, 0x00, 0x02,
                    0x08, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00,
                    0x00, 0x00, 0x00, 0x00,
                });
            CreateAndWriteStream(
                storage,
                "\u0003ObjInfo",
                new byte[] { 0x00, 0x00, 0x03, 0x00, 0x04, 0x00 });
            CreateAndWriteStream(storage, "Equation Native", equation.EquationNative);
            storage.Commit(0);
            Release(storage);
            storage = null;

            var bytes = File.ReadAllBytes(path);
            ValidateCompoundFile(bytes);
            _ = ReadMathMl(bytes);
            return bytes;
        }
        finally
        {
            Release(storage);
            TryDelete(path);
        }
    }

    public static string ReadMathMl(byte[] compoundFile)
    {
        var equationNative = ReadEquationNative(compoundFile);
        return MathTypeMtefCodec.ReadEquationNativeMathMl(equationNative);
    }

    public static byte[] ReadEquationNative(byte[] compoundFile)
    {
        ValidateCompoundFile(compoundFile);
        var path = MaterializeTemporaryCompoundFile(compoundFile, "read-native");
        try
        {
            var storage = OpenStorage(path, StgmRead | StgmShareExclusive);
            try
            {
                ValidateMathTypeStorage(storage);
                return ReadStream(storage, "Equation Native");
            }
            finally { Release(storage); }
        }
        finally { TryDelete(path); }
    }

    public static bool LooksLikeMathTypeCompoundFile(byte[] compoundFile)
    {
        if (compoundFile is null
            || compoundFile.Length < 512
            || compoundFile.Length > MaximumCompoundFileBytes)
            return false;
        if (!HasCompoundFileSignature(compoundFile)) return false;
        var path = MaterializeTemporaryCompoundFile(compoundFile, "identify");
        try
        {
            var storage = OpenStorage(path, StgmRead | StgmShareExclusive);
            try
            {
                ValidateMathTypeStorage(storage);
                return true;
            }
            catch
            {
                return false;
            }
            finally { Release(storage); }
        }
        finally { TryDelete(path); }
    }

    private static void ValidateCompoundFile(byte[] compoundFile)
    {
        if (compoundFile is null || compoundFile.Length < 512)
            throw new InvalidDataException(
                "The MathType object is too short to be a complete Compound File Binary payload.");
        if (compoundFile.Length > MaximumCompoundFileBytes)
            throw new InvalidDataException(
                $"The MathType object exceeds the supported safety limit of {MaximumCompoundFileBytes} bytes.");
        if (!HasCompoundFileSignature(compoundFile))
            throw new InvalidDataException(
                "The MathType object is not a Compound File Binary payload.");
    }

    private static bool HasCompoundFileSignature(byte[] data) =>
        data is { Length: >= 8 }
        && data[0] == 0xD0 && data[1] == 0xCF
        && data[2] == 0x11 && data[3] == 0xE0
        && data[4] == 0xA1 && data[5] == 0xB1
        && data[6] == 0x1A && data[7] == 0xE1;

    private static void ValidateMathTypeStorage(IStorageNative storage)
    {
        storage.Stat(out var stat, StatFlagNoName);
        var recognizedClass = stat.clsid == MathTypeEquationClsid
            || stat.clsid == MathTypeOleIdentity.CanonicalClsid;
        var storedUserType = TryReadStorageUserType(storage);
        var recognizedUserType = !string.IsNullOrWhiteSpace(storedUserType)
            && storedUserType!.IndexOf(
                "MathType",
                StringComparison.OrdinalIgnoreCase) >= 0;
        if (!recognizedClass && !recognizedUserType)
            throw new InvalidDataException(
                $"OLE storage class {stat.clsid:B} / user type '{storedUserType ?? "(missing)"}' is not a recognized MathType equation.");

        var native = ReadStream(storage, "Equation Native");
        if (native.Length < 34 || BitConverter.ToUInt16(native, 0) != 28)
            throw new InvalidDataException("MathType Equation Native stream has an unsupported header.");
        if (native[28] != 5)
            throw new InvalidDataException(
                $"MathType MTEF v5 required; actual={native[28]}.");
    }

    private static string? TryReadStorageUserType(IStorageNative storage)
    {
        IntPtr userTypePointer = IntPtr.Zero;
        try
        {
            var result = ReadFmtUserTypeStg(storage, out _, out userTypePointer);
            if (result < 0 || userTypePointer == IntPtr.Zero) return null;
            return Marshal.PtrToStringUni(userTypePointer);
        }
        catch
        {
            return null;
        }
        finally
        {
            if (userTypePointer != IntPtr.Zero)
                Marshal.FreeCoTaskMem(userTypePointer);
        }
    }

    private static IStorageNative OpenStorage(string path, int mode)
    {
        var result = StgOpenStorage(
            path,
            IntPtr.Zero,
            mode,
            IntPtr.Zero,
            0,
            out var storage);
        if (result < 0) Marshal.ThrowExceptionForHR(result);
        return storage;
    }

    private static byte[] ReadStream(IStorageNative storage, string name)
    {
        storage.OpenStream(
            name,
            IntPtr.Zero,
            StgmRead | StgmShareExclusive,
            0,
            out var stream);
        try
        {
            stream.Stat(out var stat, StatFlagNoName);
            if (stat.cbSize < 0 || stat.cbSize > 64L * 1024 * 1024)
                throw new InvalidDataException(
                    $"Unexpected MathType storage stream '{name}' length: {stat.cbSize}.");
            var bytes = new byte[(int)stat.cbSize];
            stream.Seek(0, 0, IntPtr.Zero);
            var readPointer = Marshal.AllocHGlobal(sizeof(int));
            try
            {
                stream.Read(bytes, bytes.Length, readPointer);
                var read = Marshal.ReadInt32(readPointer);
                if (read != bytes.Length)
                    throw new EndOfStreamException(
                        $"MathType stream '{name}' expected {bytes.Length} bytes, read {read}.");
            }
            finally { Marshal.FreeHGlobal(readPointer); }
            return bytes;
        }
        finally { Release(stream); }
    }

    private static void CreateAndWriteStream(
        IStorageNative storage,
        string name,
        byte[] data)
    {
        storage.CreateStream(
            name,
            StgmCreate | StgmReadWrite | StgmShareExclusive,
            0,
            0,
            out var stream);
        try
        {
            stream.SetSize(data.LongLength);
            stream.Seek(0, 0, IntPtr.Zero);
            var writtenPointer = Marshal.AllocHGlobal(sizeof(int));
            try
            {
                stream.Write(data, data.Length, writtenPointer);
                var written = Marshal.ReadInt32(writtenPointer);
                if (written != data.Length)
                    throw new IOException(
                        $"MathType stream '{name}' expected to write {data.Length} bytes, wrote {written}.");
            }
            finally { Marshal.FreeHGlobal(writtenPointer); }
            stream.Commit(0);
        }
        finally { Release(stream); }
    }

    private static string MaterializeTemporaryCompoundFile(
        byte[] compoundFile,
        string purpose)
    {
        var root = Path.Combine(Path.GetTempPath(), "mathset-mathtype-ole");
        Directory.CreateDirectory(root);
        var path = Path.Combine(root, $"mathtype-{purpose}-{Guid.NewGuid():N}.ole");
        File.WriteAllBytes(path, compoundFile);
        return path;
    }

    private static void TryDelete(string path)
    {
        try { if (File.Exists(path)) File.Delete(path); }
        catch { }
    }

    private static void Release(object? value)
    {
        if (value is not null && Marshal.IsComObject(value))
        {
            try { Marshal.FinalReleaseComObject(value); }
            catch { }
        }
    }

    [ComImport]
    [Guid("0000000B-0000-0000-C000-000000000046")]
    [InterfaceType(ComInterfaceType.InterfaceIsIUnknown)]
    private interface IStorageNative
    {
        void CreateStream(string name, int mode, int reserved1, int reserved2, out IStream stream);
        void OpenStream(string name, IntPtr reserved1, int mode, int reserved2, out IStream stream);
        void CreateStorage(string name, int mode, int reserved1, int reserved2, out IStorageNative storage);
        void OpenStorage(string name, IntPtr priority, int mode, IntPtr exclude, int reserved, out IStorageNative storage);
        void CopyTo(int ciidExclude, IntPtr rgiidExclude, IntPtr snbExclude, IStorageNative destination);
        void MoveElementTo(string name, IStorageNative destination, string newName, int flags);
        void Commit(int flags);
        void Revert();
        void EnumElements(int reserved1, IntPtr reserved2, int reserved3, out IntPtr enumerator);
        void DestroyElement(string name);
        void RenameElement(string oldName, string newName);
        void SetElementTimes(string name, IntPtr creation, IntPtr access, IntPtr modification);
        void SetClass(ref Guid clsid);
        void SetStateBits(int stateBits, int mask);
        void Stat(out System.Runtime.InteropServices.ComTypes.STATSTG stat, int flags);
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern uint RegisterClipboardFormatW(string format);

    [DllImport("ole32.dll", CharSet = CharSet.Unicode)]
    private static extern int WriteFmtUserTypeStg(
        IStorageNative storage,
        ushort clipboardFormat,
        string userType);

    [DllImport("ole32.dll", CharSet = CharSet.Unicode)]
    private static extern int ReadFmtUserTypeStg(
        IStorageNative storage,
        out ushort clipboardFormat,
        out IntPtr userType);

    [DllImport("ole32.dll", CharSet = CharSet.Unicode)]
    private static extern int StgCreateDocfile(
        string name,
        int mode,
        int reserved,
        out IStorageNative storage);

    [DllImport("ole32.dll", CharSet = CharSet.Unicode)]
    private static extern int StgOpenStorage(
        string name,
        IntPtr priority,
        int mode,
        IntPtr exclude,
        int reserved,
        out IStorageNative storage);
}
