<#
.SYNOPSIS
    Delete specific records by ID from a specific table in PostgreSQL with cascade support.

.DESCRIPTION
    Safely deletes one or more records by their IDs from any database table.
    For core entity tables (questions, papers, knowledge_trees, knowledge_nodes, 
    documents, ai_parse_tasks, question_collections, etc.), it automatically handles 
    foreign-key cascading and disassociation before removing the primary records.

.PARAMETER Table
    Target table name (e.g. questions, papers, knowledge_trees, users, documents, etc.)

.PARAMETER Id
    One or more IDs (UUIDs) to delete. Can be a single ID, comma-separated IDs, or array.

.PARAMETER Database
    PostgreSQL database name (default: mathset)

.PARAMETER Username
    PostgreSQL username (default: postgres)

.PARAMETER HostName
    PostgreSQL host (default: 127.0.0.1)

.PARAMETER Port
    PostgreSQL port (default: 5432)

.PARAMETER DryRun
    Preview mode. Shows how many records across all related tables will be affected without modifying the database.

.PARAMETER Force
    Skip interactive confirmation.

.EXAMPLE
    .\scripts\delete_records.ps1 -Table questions -Id "c70d4482-df30-4a22-9aa5-482c9dd1cc39" -DryRun
    Preview deletion of a question.

.EXAMPLE
    .\scripts\delete_records.ps1 -Table papers -Id "3e1b7f2a-8c9d-4e5f-a1b2-c3d4e5f6a7b8"
    Delete a paper with confirmation.

.EXAMPLE
    .\scripts\delete_records.ps1 -Table questions -Id "id1,id2,id3" -Force
    Delete multiple questions directly.
#>

[CmdletBinding()]
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Table,

    [Parameter(Mandatory = $true, Position = 1)]
    [string[]]$Id,

    [string]$Database = "mathset",
    [string]$Username = "postgres",
    [string]$HostName = "127.0.0.1",
    [int]$Port = 5432,
    [switch]$DryRun,
    [switch]$Force
)

$ErrorActionPreference = "Continue"

# Parse IDs into a flat clean array
$rawIds = @()
foreach ($item in $Id) {
    if ($item -match ',') {
        $rawIds += $item.Split(',') | ForEach-Object { $_.Trim() } | Where-Object { $_ -ne '' }
    } else {
        $trimmed = $item.Trim()
        if ($trimmed -ne '') { $rawIds += $trimmed }
    }
}

if ($rawIds.Count -eq 0) {
    Write-Error "No valid IDs provided."
    exit 1
}

Write-Host "============================================================" -ForegroundColor Cyan
Write-Host " [MathSet Database Record Deletion Script]" -ForegroundColor Cyan
Write-Host " Target Table : $Table" -ForegroundColor Cyan
Write-Host " Target IDs   : $($rawIds.Count) record(s)" -ForegroundColor Cyan
Write-Host " Database     : $Database @ $($HostName):$Port (User: $Username)" -ForegroundColor Cyan
Write-Host " Mode         : $(if ($DryRun) { 'DRY-RUN (Preview Only)' } else { 'EXECUTE' })" -ForegroundColor $(if ($DryRun) { 'Yellow' } else { 'Green' })
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

# 2. Register function if needed
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Definition
$sqlFile = Join-Path $scriptDir "delete_records.sql"

if (Test-Path $sqlFile) {
    $null = & psql -h $HostName -p $Port -U $Username -d $Database -q -v ON_ERROR_STOP=1 -f $sqlFile 2>&1
}

# Format UUID array literal: ARRAY['uuid1'::uuid, 'uuid2'::uuid]
$arrayElements = ($rawIds | ForEach-Object { "'$_'::uuid" }) -join ", "
$pgArray = "ARRAY[$arrayElements]"

# 3. Query affected records
Write-Host "`nQuerying affected records and relations..." -ForegroundColor Yellow
$previewSql = "SELECT step_name, affected_table, deleted_count FROM mathset_delete_records('$Table', $pgArray, true);"
$previewResult = & psql -h $HostName -p $Port -U $Username -d $Database -v ON_ERROR_STOP=1 -c $previewSql 2>&1

if ($LASTEXITCODE -ne 0) {
    Write-Error "Query failed: $previewResult"
    exit 1
}

Write-Host ($previewResult -join "`n")

if ($DryRun) {
    Write-Host "`n[DryRun Mode] Preview completed. No data was modified in the database." -ForegroundColor Green
    exit 0
}

# 4. Confirmation
if (-not $Force) {
    $confirmation = Read-Host "`nAre you sure you want to permanently delete the above record(s) from table [$Table]? (y/N)"
    if ($confirmation -notin @('y', 'Y', 'yes', 'YES')) {
        Write-Host "Operation cancelled." -ForegroundColor Yellow
        exit 0
    }
}

# 5. Execute deletion
Write-Host "`nExecuting deletion transaction..." -ForegroundColor Yellow
$execSql = "BEGIN; SELECT step_name, affected_table, deleted_count FROM mathset_delete_records('$Table', $pgArray, false); COMMIT;"
$execResult = & psql -h $HostName -p $Port -U $Username -d $Database -c $execSql 2>&1

if ($LASTEXITCODE -eq 0) {
    Write-Host "`nDeletion completed successfully!" -ForegroundColor Green
    Write-Host $execResult
} else {
    Write-Error "Deletion failed: $execResult"
    exit 1
}
