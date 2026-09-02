# R5 决策门探针：Word / WPS 对「w:keepNext 段落 + 紧随其后的 w:tbl」到底分不分页。
#
# 这条决定选项栅格用 w:tbl 还是退回 w:tabs，只能实测 —— 单测证明不了排版行为。
#
# 判据是**对照实验**，不是单文件读数：同一份内容跑两遍，一遍带 `w:keepNext`（探针），一遍用
# scripts/strip_keepnext.py 剥掉全部 keepNext（负对照）。每对「题号段 → 选项表」测
# `violation = 题号段页码 != 首行起始页码`（题号被单独留在上一页页尾、选项跑到下一页）。
#   探针 0 违例 且 对照 >0 违例   → keepNext 生效，w:tbl 方案成立（对照的违例数就是压力证据）
#   探针 >0 违例                  → keepNext 不被尊重 → 本任务内改用 w:tabs 制表位排选项
#   对照也是 0 违例               → 这份夹具根本没造成压力，0 违例说明不了任何事 → INCONCLUSIVE
# 行高/纵坐标一类的几何推算不用：Word 对 `cantSplit` 行与行尾标记的位置报告并不可靠（实测
# Rows.Item(1).Height 返回 9999999、行尾标记被报回行首页码），压力由对照实验直接给出更硬。
#
# 用法：
#   cargo test --lib export::docx -- --ignored                       # 产出 target/t27_keepnext_probe.docx
#   python scripts/strip_keepnext.py target/t27_keepnext_probe.docx   # 产出 *.no_keepnext.docx
#   powershell -NoProfile -ExecutionPolicy Bypass -File scripts/check_keepnext.ps1 target/t27_keepnext_probe.docx
#
# 退出码：0 = 所有可用编辑器都判定 keepNext 生效；1 = 至少一个 FAIL（改用 w:tabs）；
# 2 = 未能判定（无编辑器 / 缺对照 / 页码取不到 / 对照组同样不违例）。2 一律不算通过。
#
# 只做只读打开 + 关闭不保存，AddToRecentFiles=$false，不污染最近文件列表。
# 输出刻意全 ASCII：控制台代码页是 GBK，中文出了脚本就成乱码。
# 本文件必须带 UTF-8 BOM 保存：PowerShell 5.1 按 GBK 读无 BOM 文件，注释里的中文字节可能被解析
# 成多余的引号或花括号，整个脚本直接语法错（实测过，报 "Try statement is missing its Catch"）。

param(
    [Parameter(Mandatory = $true)][string]$Path,
    # 负对照路径；省略时按 strip_keepnext.py 的默认命名（同名 + .no_keepnext.docx）找
    [string]$Control = '',
    [string[]]$ProgIds = @('Word.Application', 'KWPS.Application')
)

$ErrorActionPreference = 'Stop'
$abs = (Resolve-Path -LiteralPath $Path).Path
if (-not $Control) { $Control = $abs -replace '\.docx$', '.no_keepnext.docx' }
$ctrlAbs = ''
if (Test-Path -LiteralPath $Control) { $ctrlAbs = (Resolve-Path -LiteralPath $Control).Path }
Write-Output "probe  =$abs"
Write-Output "control=$ctrlAbs"

$WD_PAGE_NUMBER = 3
$WD_Y_RELATIVE_TO_PAGE = 6

function Get-Pos([object]$doc, [int]$pos) {
    # 位置 → @{Page;Y;Src}。必须自己造 Range：把 $cell.Start 这类整数直接喂给 Information 会静默失败。
    # 页码取不到记 0 / Src='none'（上层据此判 INCONCLUSIVE，不会拿 0 当页码去比）。
    $out = @{ Page = 0; Y = -1.0; Src = 'none' }
    if ($pos -lt 0) { $pos = 0 }
    $range = $null
    try { $range = $doc.Range($pos, $pos) } catch { return $out }
    if ($null -eq $range) { return $out }
    try {
        $n = [int]$range.Information($WD_PAGE_NUMBER)
        if ($n -gt 0) {
            $out.Page = $n
            $out.Src = 'ok'
        }
    } catch { }
    if ($out.Src -eq 'none') {
        try {
            $pages = $range.Pages
            if ($pages -and [int]$pages.Count -ge 1) {
                $n = [int]$pages.Item(1).Number
                if ($n -gt 0) {
                    $out.Page = $n
                    $out.Src = 'pages'
                }
            }
        } catch { }
    }
    try { $out.Y = [double]$range.Information($WD_Y_RELATIVE_TO_PAGE) } catch { }
    return $out
}

function To-SafeText([string]$s) {
    if (-not $s) { return '' }
    $t = ($s -replace '[\r\n\a\t\v]', ' ').Trim()
    $t = $t -replace '\s+', ' '
    if ($t.Length -gt 20) { $t = $t.Substring(0, 20) }
    $sb = New-Object System.Text.StringBuilder
    foreach ($c in $t.ToCharArray()) {
        if ([int]$c -ge 32 -and [int]$c -le 126) { [void]$sb.Append($c) } else { [void]$sb.Append('?') }
    }
    return $sb.ToString()
}

function Measure-Doc([object]$app, [string]$file, [string]$tag) {
    # 打开一份 docx，逐对「题号段 → 表格」量页码，返回统计与明细行
    $doc = $null
    $res = @{ Ok = $false; Pairs = 0; Violations = 0; BadSrc = 0; Pages = 0; Lines = @(); Err = '' }
    try {
        # Open(FileName, ConfirmConversion, ReadOnly, AddToRecentFiles)
        $doc = $app.Documents.Open($file, $false, $true, $false)
        try { $res.Pages = [int]$doc.ComputeStatistics(2) } catch { $res.Pages = 0 }   # wdStatisticPages
        for ($i = 1; $i -le $doc.Tables.Count; $i++) {
            $tbl = $doc.Tables.Item($i)
            $tblStart = [int]$tbl.Range.Start
            $tblEnd = [int]$tbl.Range.End

            # 题号段 = 表格起点前最后一个「不在表格里」的段落
            $pre = $doc.Range(0, $tblStart)
            $paras = $pre.Paragraphs
            if ([int]$paras.Count -lt 1) { continue }
            $stem = $null
            $j = [int]$paras.Count
            # 用 $null 比较而不是 -not：Word COM 对象的布尔转换不随引用变化，-not 判不出「已找到」
            while ($j -ge 1 -and $null -eq $stem) {
                $cand = $paras.Item($j)
                $inTable = $false
                try { $inTable = ([int]$cand.Range.Cells.Count -gt 0) } catch { }
                if (-not $inTable) { $stem = $cand } else { $j-- }
            }
            if ($null -eq $stem) { continue }
            $stemText = To-SafeText ([string]$stem.Range.Text)
            # 只统计「题号段 → 表格」对（题号以数字开头），表头信息表不参与判定
            if ($stemText -notmatch '^[0-9]') { continue }
            $res.Pairs++

            $mStem = Get-Pos $doc ([int]$stem.Range.End - 1)
            $mRow = @{ Page = 0; Y = -1.0; Src = 'none' }
            try {
                $mRow = Get-Pos $doc ([int]$tbl.Rows.Item(1).Range.Start)
            } catch { }
            $mEnd = Get-Pos $doc ($tblEnd - 1)
            foreach ($m in @($mStem, $mRow, $mEnd)) { if ($m.Src -ne 'ok') { $res.BadSrc++ } }

            $violation = ($mStem.Page -ne $mRow.Page)
            if ($violation) { $res.Violations++ }
            $res.Lines += ("{0} table={1,-3} stemPg={2,-3} row1Pg={3,-3} tblEndPg={4,-3} stemY={5,-4} violation={6,-6} stem='{7}'" -f `
                $tag, $i, $mStem.Page, $mRow.Page, $mEnd.Page, [math]::Round($mStem.Y, 0), $violation, $stemText)
        }
        $res.Ok = $true
    }
    catch {
        $res.Err = $_.Exception.Message
    }
    finally {
        try { if ($doc) { $doc.Close($false) } } catch { }
        try { if ($doc) { [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($doc) } } catch { }
    }
    return $res
}

$anyFail = $false
$anyInconclusive = $false
$anyEditor = $false

foreach ($progId in $ProgIds) {
    if (-not (Test-Path "Registry::HKEY_CLASSES_ROOT\$progId")) {
        Write-Output "SKIP  $progId (not registered)"
        continue
    }
    if (-not $ctrlAbs) {
        Write-Output "INCONCLUSIVE no negative control file - run scripts/strip_keepnext.py first"
        $anyInconclusive = $true
        continue
    }
    $anyEditor = $true

    $app = $null
    $probe = $null
    $ctrl = $null
    try {
        $app = New-Object -ComObject $progId
        try { $app.Visible = $false } catch { }
        try { $app.DisplayAlerts = 0 } catch { }
        $probe = Measure-Doc $app $abs 'probe  '
        $ctrl = Measure-Doc $app $ctrlAbs 'control'
    }
    catch {
        Write-Output "FAIL  $progId cannot probe: $($_.Exception.Message)"
        $anyFail = $true
    }
    finally {
        try { if ($app) { $app.Quit() } } catch { }
        if ($app) {
            try { [void][System.Runtime.InteropServices.Marshal]::ReleaseComObject($app) } catch { }
        }
    }

    if ($null -eq $probe -or $null -eq $ctrl) { continue }

    Write-Output "=== $progId probePages=$($probe.Pages) controlPages=$($ctrl.Pages)"
    foreach ($line in $probe.Lines) { Write-Output $line }
    foreach ($line in $ctrl.Lines) { Write-Output $line }
    Write-Output "summary $progId probe pairs=$($probe.Pairs) violations=$($probe.Violations) | control pairs=$($ctrl.Pairs) violations=$($ctrl.Violations) | badSrc=$($probe.BadSrc + $ctrl.BadSrc)"

    if (-not $probe.Ok -or -not $ctrl.Ok) {
        $err = if ($probe.Err) { $probe.Err } else { $ctrl.Err }
        Write-Output "FAIL  $progId cannot measure: $err"
        $anyFail = $true
    }
    elseif ($probe.BadSrc -gt 0 -or $ctrl.BadSrc -gt 0) {
        Write-Output "INCONCLUSIVE $progId page numbers not available via Information() for all probes"
        $anyInconclusive = $true
    }
    elseif ($probe.Pairs -eq 0 -or $probe.Pairs -ne $ctrl.Pairs) {
        Write-Output "INCONCLUSIVE $progId pair count mismatch probe=$($probe.Pairs) control=$($ctrl.Pairs)"
        $anyInconclusive = $true
    }
    elseif ($probe.Violations -gt 0) {
        Write-Output "FAIL  $progId keepNext ignored: stem orphaned on $($probe.Violations)/$($probe.Pairs) pairs"
        $anyFail = $true
    }
    elseif ($ctrl.Violations -eq 0) {
        Write-Output "INCONCLUSIVE $progId control also clean - fixture created no pressure, 0 violations means nothing"
        $anyInconclusive = $true
    }
    else {
        Write-Output "PASS  $progId keepNext works: control orphaned $($ctrl.Violations)/$($ctrl.Pairs), probe orphaned 0/$($probe.Pairs)"
    }
}

if ($anyFail) { exit 1 }
if (-not $anyEditor -or $anyInconclusive) {
    Write-Output 'NOT VERIFIED - keepNext behaviour unknown, do not treat as pass'
    exit 2
}
exit 0
