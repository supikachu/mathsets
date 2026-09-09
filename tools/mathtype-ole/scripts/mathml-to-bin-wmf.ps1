# MathML → ole.bin (mathtype-ole) → WMF (mt_converter_portable preferred)
param(
    [Parameter(Mandatory = $true)][string]$MathMlPath,
    [Parameter(Mandatory = $true)][string]$OutDir,
    [switch]$Inline,
    [double]$FontSizePt = 0
)

$ErrorActionPreference = "Stop"
$oleRoot = Resolve-Path (Join-Path $PSScriptRoot "..")
$tools = Resolve-Path (Join-Path $oleRoot "..")
$portableRoot = Join-Path $tools "mt_converter_portable"
$cli = Join-Path $oleRoot "MathTypeOle.Cli\bin\Release\net472\MathTypeOle.Cli.exe"

if (-not (Test-Path $cli)) {
    Write-Host "Building mathtype-ole..."
    Push-Location $oleRoot
    try { dotnet build MathTypeOle.sln -c Release } finally { Pop-Location }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$base = [IO.Path]::GetFileNameWithoutExtension($MathMlPath)
if ($base.EndsWith(".mathml", [StringComparison]::OrdinalIgnoreCase)) {
    $base = [IO.Path]::GetFileNameWithoutExtension($base)
}
$binPath = Join-Path $OutDir "$base.bin"
$wmfPath = Join-Path $OutDir "$base.wmf"

$extra = @()
if ($Inline) { $extra += "--inline" }
if ($FontSizePt -gt 0) { $extra += @("--font-size", "$FontSizePt") }

& $cli $MathMlPath $binPath --verify @extra
if ($LASTEXITCODE -ne 0) { throw "bin conversion failed" }

& $cli --from-ole $binPath -o $wmfPath
if ($LASTEXITCODE -ne 0) { throw "wmf conversion failed" }

Write-Host "OK: $binPath"
Write-Host "OK: $wmfPath"
if (Test-Path (Join-Path $portableRoot "mt_converter.exe")) {
    Write-Host "(WMF engine: portable converter if discovered by CLI)"
}
