# 创建独立测试库并执行迁移（与 .env 中 DATABASE_URL_TEST 对应）
$ErrorActionPreference = "Stop"

$DevUrl = $env:DATABASE_URL
if (-not $DevUrl) {
    if (Test-Path ".env") {
        Get-Content ".env" | ForEach-Object {
            if ($_ -match '^\s*DATABASE_URL=(.+)$') { $DevUrl = $matches[1].Trim() }
        }
    }
}
if (-not $DevUrl) {
    Write-Error "请设置 DATABASE_URL 或在项目根目录配置 .env"
}

$TestUrl = $env:DATABASE_URL_TEST
if (-not $TestUrl) {
    if (Test-Path ".env") {
        Get-Content ".env" | ForEach-Object {
            if ($_ -match '^\s*DATABASE_URL_TEST=(.+)$') { $TestUrl = $matches[1].Trim() }
        }
    }
}
if (-not $TestUrl) {
    $TestUrl = ($DevUrl -replace '/[^/]+$', '/mathset_test')
}

if ($TestUrl -match '/([^/?]+)(\?.*)?$') {
    $DbName = $matches[1]
} else {
    Write-Error "无法从连接串解析数据库名: $TestUrl"
}

$AdminUrl = ($DevUrl -replace '/[^/]+$', '/postgres')

Write-Host "创建测试库 $DbName（若已存在则跳过）..."
psql $AdminUrl -v ON_ERROR_STOP=1 -c "SELECT 1 FROM pg_database WHERE datname = '$DbName'" | Out-Null
$exists = psql $AdminUrl -tAc "SELECT 1 FROM pg_database WHERE datname = '$DbName'"
if ($exists.Trim() -ne "1") {
    psql $AdminUrl -v ON_ERROR_STOP=1 -c "CREATE DATABASE `"$DbName`""
}

Write-Host "对测试库执行迁移: $TestUrl"
$env:DATABASE_URL = $TestUrl
sqlx migrate run

Write-Host "完成。请在 .env 中设置: DATABASE_URL_TEST=$TestUrl"
