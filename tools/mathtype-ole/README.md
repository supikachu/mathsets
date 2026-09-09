# MathType OLE kit (mathset)

Minimal extract from [VisualTeX](https://github.com/paulhe666/visualtex) (MIT):
**MathML → MathType `ole.bin`**, plus **MathML/MTEF/`ole.bin` → authentic WMF**
(preferred: `tools/mt_converter_portable`; fallback: in-process `MT6.dll`).

## Layout

| Path | Role |
|------|------|
| `MathTypeOle.Core/` | MTEF codec + CFB writer + WMF (portable bridge / MT6) |
| `MathTypeOle.Cli/` | Windows **x64** console |
| `scripts/mathml-to-bin-wmf.ps1` | End-to-end MathML → bin + wmf |
| `LICENSE` | Upstream VisualTeX MIT license (required) |
| `../mt_converter_portable/` | Portable bin/MTEF → WMF + baseline (sibling tool) |

**Not included:** Word VSTO, clipboard paste, OOXML `w:object` assembly, macOS/Tauri editor.

## WMF engines

1. **Portable (preferred)** — `tools/mt_converter_portable/mt_converter.exe` + local `mathtype_core/`  
2. **In-process MT6** — system `MathType\System\64\MT6.dll`

`ole.bin` generation does **not** require MathType. **WMF** needs portable core or a system install.

| Variable | Meaning |
|----------|---------|
| `MATHTYPE_WMF_ENGINE` | `portable` (default if found) or `mt6` |
| `MATHTYPE_PORTABLE_CONVERTER` | Path to `mt_converter.exe` |
| `MATHTYPE_PORTABLE_CORE` | Path to `mathtype_core` |
| `MATHTYPE_MT6_DIR` / `MATHTYPE_MT6_DLL` | In-process MT6 override |

See also `../mt_converter_portable/README.md`.

## Build (Windows)

```powershell
cd tools/mathtype-ole
dotnet build MathTypeOle.sln -c Release
```

Requires .NET SDK + net472 targeting pack. Output is **x64**.

## Testing

See **[TESTING.md](./TESTING.md)** for the smoke checklist (bin + WMF).  
Fixtures live under `samples/*.mathml.xml`.

```powershell
dotnet build MathTypeOle.sln -c Release
$cli = ".\MathTypeOle.Cli\bin\Release\net472\MathTypeOle.Cli.exe"
& $cli .\samples\01-frac.mathml.xml .\samples\out\01-frac.bin --verify
& $cli .\samples\01-frac.mathml.xml .\samples\out\01-frac.wmf --wmf
& $cli --from-ole .\samples\out\01-frac.bin -o .\samples\out\01-from-ole.wmf
& $cli --demo-docx -o .\samples\out\mathtype-demo.docx
```

## CLI

```powershell
# ole.bin only
.\MathTypeOle.Cli\bin\Release\net472\MathTypeOle.Cli.exe input.mathml.xml out.bin --verify

# authentic MathType WMF
.\MathTypeOle.Cli\...\MathTypeOle.Cli.exe input.mathml.xml out.wmf --wmf

# both
.\MathTypeOle.Cli\...\MathTypeOle.Cli.exe input.mathml.xml out.bin --bin --wmf

# demo Word document with 4 MathType OLE equations
.\MathTypeOle.Cli\...\MathTypeOle.Cli.exe --demo-docx -o samples\out\mathtype-demo.docx

# from existing assets
.\MathTypeOle.Cli\...\MathTypeOle.Cli.exe --from-ole out.bin -o preview.wmf
.\MathTypeOle.Cli\...\MathTypeOle.Cli.exe --from-mtef eqn.mtef -o preview.wmf
```

## Library

```csharp
using VisualTeX.WordVsto;

byte[] oleBin = MathTypeOleStorage.CreateStandaloneCompoundFile(mathMl, inline: false);
MathTypeWmfConverter.WriteWmfFromMathMl(mathMl, @"C:\temp\eq.wmf", inline: false);
MathTypeWmfConverter.WriteWmfFromOleBin(oleBin, @"C:\temp\eq.wmf");

MathTypeDocxWriter.WriteDemoDocxFromMathMl(
    new[]
    {
        ("frac", mathMl, false),
    },
    @"C:\temp\demo.docx");
```

## Upstream sources

Kept from VisualTeX `apps/windows/src-windows/VisualTeX.WordVsto/`:

- `MathTypeMtefCodec.cs` / `MathTypeMtefCodec.FontSize.cs`
- Slimmed create-path of `MathTypeOleStorage` + identity constants from `MathTypeOleInterop`

WMF path uses MathType **MT6.dll** SDK APIs (`MTXFormEqn`), not VisualTeX preview code.
