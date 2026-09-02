# T2.6 DoD 探针：最小 docx 必须被 Word 与 WPS **正常打开**。
#
# 单测能证明包结构合法（三条不变量），证明不了 Word/WPS 认这份结构 —— Word 判损坏的
# 表现往往是整个文件打不开，而不是缺一块内容。所以这里用 COM 真开一遍：
# Documents.Open 抛异常即视为不通过。
#
# 用法：
#   cargo test --lib export::docx -- --ignored        # 产出 target/t26_probe.docx
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check_docx_opens.ps1 target/t26_probe.docx
#   # T4.12 纸张探针：顺手把 Word 眼里的页面尺寸与栏数对回 spec
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check_docx_opens.ps1 `
#     target/t412_a3_fold_exam.docx -ExpectPageMm 420x297 -ExpectColumns 2
#
# 退出码：0 = 全部可用编辑器都打开成功；1 = 至少一个打开失败；2 = 没有可用编辑器（未验证，不算通过）。
#
# 只做只读打开 + 关闭不保存，且 AddToRecentFiles=$false，不污染用户的最近文件列表。
# Word / WPS 的 COM 服务器是 single-use，New-Object 会另起进程，不影响用户已开的窗口。
#
# 输出刻意用 ASCII：PowerShell 5.1 按 ANSI 读无 BOM 脚本，中文只在注释里出现，不参与输出。

param(
    [Parameter(Mandatory = $true)][string]$Path,
    [string[]]$ProgIds = @('Word.Application', 'KWPS.Application'),
    # -1 = 不校验公式对象数（复用于 T2.7 的探针）
    [int]$ExpectedOMaths = -1,
    # 正文里应当出现的文字，用于确认内容真的渲染出来了
    [string]$ExpectedText = '',
    # 期望页面尺寸（mm，形如 420x297）；空 = 只报告不判定。容差 1mm：twips 取整过一道
    [string]$ExpectPageMm = '',
    # 期望栏数（`-ExpectColumns 2`）；-1 = 只报告不判定
    [int]$ExpectColumns = -1
)

$ErrorActionPreference = 'Stop'
$abs = (Resolve-Path -LiteralPath $Path).Path
Write-Output "target=$abs"

$missing = 0
$failed = 0

function Get-Prop($obj, $name) {
    try { return $obj.$name } catch { return $null }
}

foreach ($progId in $ProgIds) {
    $regPath = "Registry::HKEY_CLASSES_ROOT\$progId"
    if (-not (Test-Path $regPath)) {
        Write-Output "SKIP  $progId (not registered)"
        $missing++
        continue
    }

    $app = $null
    $doc = $null
    try {
        $app = New-Object -ComObject $progId
        try { $app.Visible = $false } catch { }
        try { $app.DisplayAlerts = 0 } catch { }        # wdAlertsNone

        # Open(FileName, ConfirmConversion, ReadOnly, AddToRecentFiles)
        $doc = $app.Documents.Open($abs, $false, $true, $false)

        $text = [string](Get-Prop $doc.Content 'Text')
        $clean = ($text -replace '[\r\n\a\t]', ' ').Trim()
        # WPS 未必实现 OMaths / ComputeStatistics，取不到时打 '?' 而不是让探针崩掉
        try { $pages = $doc.ComputeStatistics(2) } catch { $pages = '?' }   # wdStatisticPages
        try { $omaths = $doc.OMaths.Count } catch { $omaths = '?' }

        # 页面几何（T4.12）：Word 报 point，1mm = 72/25.4 pt。取不到时留 '?'，由下面的期望值决定算不算失败
        $pw = '?'; $ph = '?'; $cols = '?'
        try {
            $ps = $doc.PageSetup
            $pw = [math]::Round([double]$ps.PageWidth * 25.4 / 72, 1)
            $ph = [math]::Round([double]$ps.PageHeight * 25.4 / 72, 1)
            try { $cols = $ps.TextColumns.Count } catch { }
        } catch { }

        Write-Output "PASS  $progId pages=$pages omaths=$omaths chars=$($clean.Length)"
        Write-Output "      page=${pw}x${ph}mm columns=$cols"
        Write-Output "      text=$clean"

        if ($ExpectedOMaths -ge 0 -and ($omaths -isnot [int] -or $omaths -ne $ExpectedOMaths)) {
            Write-Output "FAIL  $progId omaths=$omaths expected=$ExpectedOMaths"
            $failed++
        }
        if ($ExpectedText -and -not $clean.Contains($ExpectedText)) {
            Write-Output "FAIL  $progId rendered text missing '$ExpectedText'"
            $failed++
        }
        if ($ExpectPageMm) {
            if ($pw -isnot [double]) {
                Write-Output "FAIL  $progId PageSetup unavailable, expected $ExpectPageMm"
                $failed++
            } else {
                $want = $ExpectPageMm -split 'x'
                if ([math]::Abs($pw - [double]$want[0]) -gt 1 -or [math]::Abs($ph - [double]$want[1]) -gt 1) {
                    Write-Output "FAIL  $progId page=${pw}x${ph}mm expected=$ExpectPageMm"
                    $failed++
                }
            }
        }
        if ($ExpectColumns -ge 0 -and ($cols -isnot [int] -or $cols -ne $ExpectColumns)) {
            Write-Output "FAIL  $progId columns=$cols expected=$ExpectColumns"
            $failed++
        }
    }
    catch {
        Write-Output "FAIL  $progId cannot open: $($_.Exception.Message)"
        $failed++
    }
    finally {
        try { if ($doc) { $doc.Close($false) } } catch { }
        try { if ($app) { $app.Quit() } } catch { }
        foreach ($o in @($doc, $app)) {
            if ($o) {
                try { [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($o) } catch { }
            }
        }
    }
}

if ($failed -gt 0) { exit 1 }
if ($missing -eq $ProgIds.Count) {
    Write-Output 'ERROR no editor available - DoD NOT verified'
    exit 2
}
exit 0
