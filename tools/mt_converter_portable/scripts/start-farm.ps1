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

if (-not (Test-Path $exe)) {
    Write-Error "mt_converter.exe not found at $exe. Place the portable binary next to mathtype_core/."
}

Write-Host "Stopping existing mt_converter / MathType processes..."
Get-Process mt_converter, MathType, MathTypeLib -ErrorAction SilentlyContinue | Stop-Process -Force
Start-Sleep -Seconds 1

if ($StopOnly) {
    Write-Host "Stopped. Exiting (-StopOnly)."
    exit 0
}

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
