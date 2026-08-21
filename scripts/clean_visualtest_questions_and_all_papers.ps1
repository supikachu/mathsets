[CmdletBinding()]
param(
    [string]$Database = "mathset",
    [string]$Username = "postgres",
    [string]$HostName = "127.0.0.1",
    [int]$Port = 5432,
    [string]$TargetUser = "visualtest",
    [int]$Days = 7,
    [switch]$DryRun,
    [switch]$Force
)

$ErrorActionPreference = "Stop"

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host " MathSet Data Cleanup Script" -ForegroundColor Cyan
Write-Host " Target: User [$TargetUser] recent $Days days questions + all papers" -ForegroundColor Cyan
Write-Host " Database: $Database @ $($HostName):$Port (User: $Username)" -ForegroundColor Cyan
Write-Host "============================================================" -ForegroundColor Cyan

# 1. Check psql
$psqlPath = Get-Command psql -ErrorAction SilentlyContinue
if (-not $psqlPath) {
    $commonPaths = @(
        "C:\Program Files\PostgreSQL\17\bin\psql.exe",
        "C:\Program Files\PostgreSQL\16\bin\psql.exe",
        "C:\Program Files\PostgreSQL\15\bin\psql.exe"
    )
    foreach ($p in $commonPaths) {
        if (Test-Path $p) {
            $env:Path += ";$(Split-Path $p)"
            $psqlPath = Get-Command psql -ErrorAction SilentlyContinue
            break
        }
    }
}

if (-not $psqlPath) {
    Write-Error "psql command not found. Please ensure PostgreSQL bin directory is in PATH."
    exit 1
}

# 2. Query statistics
$countSql = "SELECT (SELECT COUNT(*) FROM questions q JOIN users u ON q.creator_id = u.id WHERE u.username = '$TargetUser' AND q.created_at >= NOW() - INTERVAL '$Days days') as target_q, (SELECT COUNT(*) FROM papers) as total_papers, (SELECT COUNT(*) FROM paper_questions) as total_paper_questions, (SELECT COUNT(*) FROM questions) as total_questions;"

Write-Host ""
Write-Host "Querying database statistics..." -ForegroundColor Yellow
$statsResult = & psql -h $HostName -p $Port -U $Username -d $Database -t -A -F "," -c $countSql 2>&1

if ($LASTEXITCODE -ne 0) {
    Write-Error "Failed to connect to database: $statsResult"
    exit 1
}

$parts = $statsResult.Trim().Split(',')
if ($parts.Count -lt 4) {
    Write-Error "Failed to parse query output: $statsResult"
    exit 1
}

$targetQuestions = [int]$parts[0]
$totalPapers = [int]$parts[1]
$totalPaperQuestions = [int]$parts[2]
$totalQuestions = [int]$parts[3]

Write-Host "  * Target questions created by [$TargetUser] in last $Days days: " -NoNewline
Write-Host "$targetQuestions" -ForegroundColor Red
Write-Host "  * Total papers to delete: " -NoNewline
Write-Host "$totalPapers" -ForegroundColor Red
Write-Host "  * Total paper_questions mapping records to delete: " -NoNewline
Write-Host "$totalPaperQuestions" -ForegroundColor Red
Write-Host "  * Total questions currently in database: $totalQuestions" -ForegroundColor Gray

if ($DryRun) {
    Write-Host ""
    Write-Host "[DryRun Mode] Preview only. No changes were made to the database." -ForegroundColor Green
    exit 0
}

if ($targetQuestions -eq 0 -and $totalPapers -eq 0) {
    Write-Host ""
    Write-Host "No matching questions or papers found to clean." -ForegroundColor Green
    exit 0
}

# 3. Confirmation
if (-not $Force) {
    $confirmation = Read-Host "`nAre you sure you want to delete $targetQuestions questions and all $totalPapers papers from [$Database]? (y/N)"
    if ($confirmation -notin @('y', 'Y', 'yes', 'YES')) {
        Write-Host "Operation cancelled." -ForegroundColor Yellow
        exit 0
    }
}

# 4. Execute SQL
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$sqlFile = Join-Path $scriptDir "clean_visualtest_questions_and_all_papers.sql"

if (-not (Test-Path $sqlFile)) {
    Write-Error "SQL script not found: $sqlFile"
    exit 1
}

Write-Host ""
Write-Host "Executing cleanup transaction..." -ForegroundColor Yellow
$result = & psql -h $HostName -p $Port -U $Username -d $Database -f $sqlFile 2>&1

if ($LASTEXITCODE -eq 0) {
    Write-Host ""
    Write-Host "Cleanup completed successfully!" -ForegroundColor Green
    Write-Host $result
} else {
    Write-Error "Cleanup execution failed: $result"
    exit 1
}
