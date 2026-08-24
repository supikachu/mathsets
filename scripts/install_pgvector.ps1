# pgvector 快速安装与数据库启用脚本（需要管理员权限拷贝文件）
$ErrorActionPreference = "Stop"

$src = "C:\pgvector"
$pg = "C:\Program Files\PostgreSQL\17"

Write-Host "1. 正在将 pgvector 扩展文件拷贝至 PostgreSQL 17 目录..." -ForegroundColor Cyan
Copy-Item "$src\vector.dll" "$pg\lib\" -Force
Copy-Item "$src\vector.control" "$pg\share\extension\" -Force
Copy-Item "$src\vector--*.sql" "$pg\share\extension\" -Force
if (-not (Test-Path "$pg\include\server\extension\vector")) {
    New-Item -ItemType Directory -Path "$pg\include\server\extension\vector" -Force | Out-Null
}
Copy-Item "$src\src\*.h" "$pg\include\server\extension\vector\" -Force -ErrorAction SilentlyContinue

Write-Host "2. 启用数据库 vector 扩展..." -ForegroundColor Cyan
& psql -U postgres -d postgres -c "CREATE EXTENSION IF NOT EXISTS vector;"
& psql -U postgres -d mathset -c "CREATE EXTENSION IF NOT EXISTS vector;"
& psql -U postgres -d mathset_test -c "CREATE EXTENSION IF NOT EXISTS vector;"

Write-Host "3. 验证扩展安装版本..." -ForegroundColor Cyan
& psql -U postgres -d mathset -c "SELECT extname, extversion FROM pg_extension WHERE extname = 'vector';"
& psql -U postgres -d mathset_test -c "SELECT extname, extversion FROM pg_extension WHERE extname = 'vector';"

Write-Host "4. 执行数据库迁移以创建向量表和 HNSW 索引..." -ForegroundColor Cyan
$env:DATABASE_URL = "postgres://postgres@127.0.0.1/mathset"
& sqlx migrate run
$env:DATABASE_URL = "postgres://postgres@127.0.0.1/mathset_test"
& sqlx migrate run

Write-Host "`n[SUCCESS] pgvector 安装与数据库启用完成！" -ForegroundColor Green
