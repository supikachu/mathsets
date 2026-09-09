# Start N isolated mt_converter --serve processes (one MT6 per process).
# Usage:
#   .\scripts\start-farm.ps1
#   .\scripts\start-farm.ps1 -Count 4 -StartPort 8091
#   .\scripts\start-farm.ps1 -StopOnly

param(
    [int]$Count = 4,
    [int]$StartPort = 8091,
    [switch]$StopOnly
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$exe = Join-Path $root "mt_converter.exe"
$fontsDir = Join-Path $root "mathtype_core\Fonts"

if (-not (Test-Path $exe)) {
    Write-Error "mt_converter.exe not found at $exe. Place the portable binary next to mathtype_core/."
}

if (-not ("MtFarmFonts" -as [type])) {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class MtFarmFonts {
  [DllImport("gdi32.dll", CharSet = CharSet.Unicode)]
  public static extern int AddFontResourceEx(string file, uint fl, IntPtr pdv);
  [DllImport("gdi32.dll", CharSet = CharSet.Unicode)]
  public static extern int RemoveFontResourceEx(string file, uint fl, IntPtr pdv);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern IntPtr SendMessageTimeout(IntPtr hWnd, uint Msg, IntPtr wParam, IntPtr lParam, uint flags, uint timeout, out IntPtr result);
}
"@
}

function Broadcast-FontChange {
    $ignored = [IntPtr]::Zero
    [void][MtFarmFonts]::SendMessageTimeout(
        [IntPtr]0xFFFF, [uint32]0x1D, [IntPtr]::Zero, [IntPtr]::Zero,
        [uint32]0, [uint32]1000, [ref]$ignored)
}

function Unload-CoreFonts {
    if (-not (Test-Path $fontsDir)) { return }
    $n = 0
    Get-ChildItem -Path $fontsDir -Include *.ttf,*.otf -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
        # Remove repeatedly — AddFontResourceEx is refcounted.
        while ([MtFarmFonts]::RemoveFontResourceEx($_.FullName, [uint32]0, [IntPtr]::Zero) -gt 0) { $n++ }
    }
    Broadcast-FontChange
    Write-Host "Unloaded core font resources ($n remove calls)"
}

function Load-CoreFonts {
    if (-not (Test-Path $fontsDir)) { return }
    $n = 0
    Get-ChildItem -Path $fontsDir -Include *.ttf,*.otf -Recurse -ErrorAction SilentlyContinue | ForEach-Object {
        if ([MtFarmFonts]::AddFontResourceEx($_.FullName, [uint32]0, [IntPtr]::Zero) -gt 0) { $n++ }
    }
    Broadcast-FontChange
    Write-Host "Preloaded $n fonts from $fontsDir for MathType WMF rendering"
}

Write-Host "Stopping existing mt_converter / MathType processes..."
Get-Process mt_converter, MathType, MathTypeLib -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1
Unload-CoreFonts

if ($StopOnly) {
    Write-Host "Stopped. Exiting (-StopOnly)."
    exit 0
}

# Session-wide font load so MathType.exe (MTXFormEqn child) resolves Euclid/MT Extra.
Load-CoreFonts

$ports = @()
for ($i = 0; $i -lt $Count; $i++) {
    $ports += ($StartPort + $i)
}

Write-Host "Starting farm on ports: $($ports -join ', ')"
foreach ($port in $ports) {
    Start-Process -FilePath $exe -ArgumentList @("--serve", "--port", "$port") -WorkingDirectory $root -WindowStyle Minimized
    Write-Host "  spawned --serve --port $port"
    Start-Sleep -Milliseconds 400
}

Start-Sleep -Seconds 2
Write-Host ""
Write-Host "Health checks:"
foreach ($port in $ports) {
    $url = "http://127.0.0.1:$port/health"
    try {
        $r = Invoke-RestMethod -Uri $url -TimeoutSec 5
        Write-Host "  OK  $url  ($($r | ConvertTo-Json -Compress))"
    } catch {
        Write-Host "  FAIL $url  $_"
    }
}

Write-Host ""
Write-Host "Set in .env (example):"
$urls = ($ports | ForEach-Object { "http://127.0.0.1:$_" }) -join ","
Write-Host "MATHTYPE_CONVERT_URLS=$urls"
Write-Host ""
Write-Host "Then restart the mathset backend."
