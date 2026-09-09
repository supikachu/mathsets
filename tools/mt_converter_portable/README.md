# mt_converter_portable

Portable **MathType `.bin` / MTEF → placeable `.wmf`** converter with **baseline** extraction.

Used by the mathset Formula Worker (`MATHTYPE_CONVERT_URL`) and by `tools/mathtype-ole` as the preferred WMF engine.

## Layout

```text
mt_converter_portable/
  mt_converter.exe      # CLI + HTTP (--serve, default port 8099) — gitignored
  mathtype_core/        # LOCAL ONLY — MT6.dll, MathType.exe, Fonts/ — gitignored
  python/               # Optional source (rebuild with build.py)
  samples_wmf/          # optional smoke outputs — gitignored
  README.md
```

Copy a fresh build from `Desktop/tools/dist/mt_converter_portable/` after upgrading the converter.

## Start HTTP microservice (recommended for题库)

**题库主进程不会自动启动本 exe。** 须先起本服务，再 `cargo run` 题库。  
完整顺序见仓库根文档 [`docs/MathType公式资产接入.md`](../../docs/MathType公式资产接入.md)「启动流程」。

```powershell
cd tools\mt_converter_portable
.\mt_converter.exe --serve --port 8099
```

题库 `.env` 中设置 `MATHTYPE_CONVERT_URL=http://127.0.0.1:8099` 后，Formula Worker 才会把 pending 公式打到本服务的 `/convert_batch`。

### `GET /health`

```json
{ "status": "ok", "mathtype_core": "...", "fonts_count": 23 }
```

### `POST /convert`

```json
{ "bin_base64": "<ole.bin or MTEF base64>" }
```

Response includes `wmf_base64`, `baseline_offset_pt`, `width_pt`, `height_pt`, `code`, `method`.

### `POST /convert_batch` (一题多式)

```json
{
  "equations": [
    { "id": "stem:0", "bin_base64": "..." },
    { "id": "stem:1", "bin_base64": "..." }
  ]
}
```

```json
{
  "code": 0,
  "total": 2,
  "results": [
    {
      "id": "stem:0",
      "wmf_base64": "...",
      "baseline_offset_pt": 12.0,
      "width_pt": 12.0,
      "height_pt": 31.0,
      "code": 0,
      "method": "direct_mt6_dll"
    }
  ]
}
```

Batch runs on a single STA worker thread (MathType COM-safe). Prefer one batch per question at submit time.

## CLI

```powershell
.\mt_converter.exe -i formula.bin -o out.wmf --json --print-baseline
.\mt_converter.exe --batch-dir .\inputs --output-dir .\outputs
```

## Env (题库 / mathtype-ole)

| Variable | Meaning |
|----------|---------|
| `MATHTYPE_CONVERT_URL` | e.g. `http://127.0.0.1:8099` — Rust Formula Worker |
| `MATHTYPE_PORTABLE_CONVERTER` | Full path to `mt_converter.exe` |
| `MATHTYPE_PORTABLE_CORE` | Path to `mathtype_core` |
| `MATHTYPE_WMF_ENGINE` | `portable` (default if found) or `mt6` |
| `MATHTYPE_OLE_CLI` | Path to `MathTypeOle.Cli.exe` (MathML→bin) |

## License / git

`mathtype_core/` and `mt_converter.exe` are **not** committed. Do not redistribute Wiris binaries without rights.
