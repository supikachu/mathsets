# 校验 assets/fonts/ 下的思源字体已真实落盘（不是 git-lfs 占位文件）
# 用途：拦截「克隆者未安装 git-lfs → 拿到 132 字节 pointer → 中文豆腐块」这一静默失败
$ErrorActionPreference = "Stop"

$SCRIPT_DIR = Split-Path -Parent $MyInvocation.MyCommand.Path
$FONT_DIR = [System.IO.Path]::GetFullPath((Join-Path $SCRIPT_DIR "..\assets\fonts"))
$MIN_BYTES = 1MB
$LFS_POINTER_PREFIX = "version https://git-lfs.github.com/spec/v1"

$EXPECTED = @(
    @{ Label = "思源宋体 SC Regular"; Pattern = "SourceHanSerifSC-Regular.*" },
    @{ Label = "思源宋体 SC Bold"; Pattern = "SourceHanSerifSC-Bold.*" },
    @{ Label = "思源黑体 SC Regular"; Pattern = "SourceHanSansSC-Regular.*" },
    @{ Label = "思源黑体 SC Bold"; Pattern = "SourceHanSansSC-Bold.*" }
)

function Show-FixSteps {
    Write-Host ""
    Write-Host "字体校验未通过。修复步骤：" -ForegroundColor Yellow
    Write-Host "  1. 安装 git-lfs（https://git-lfs.com），然后执行 git lfs install"
    Write-Host "  2. 拉取字体：git lfs pull"
    Write-Host "  3. 重新运行：scripts\check_fonts.ps1"
    Write-Host "详见 docs 目录下《导出引擎与排版系统_实施计划.md》的「十三、字体与 git-lfs 落地」"
}

Write-Host "检查字体目录: $FONT_DIR"

if (-not (Test-Path $FONT_DIR)) {
    Write-Host "[缺失] 字体目录不存在: $FONT_DIR" -ForegroundColor Red
    Show-FixSteps
    exit 1
}

$failed = $false

foreach ($font in $EXPECTED) {
    $found = @(Get-ChildItem -Path $FONT_DIR -Filter $font.Pattern -File -ErrorAction SilentlyContinue)

    if ($found.Count -eq 0) {
        Write-Host "[缺失] $($font.Label) —— 未找到匹配 $($font.Pattern) 的文件" -ForegroundColor Red
        $failed = $true
        continue
    }

    $file = $found[0]
    $size = $file.Length

    if ($size -lt $MIN_BYTES) {
        $head = Get-Content -Path $file.FullName -TotalCount 1 -ErrorAction SilentlyContinue
        if ($head -and $head.StartsWith($LFS_POINTER_PREFIX)) {
            Write-Host "[LFS pointer] $($font.Label) —— $($file.Name) 仍是 git-lfs 占位文件（$size 字节），未拉取真实字体" -ForegroundColor Red
        } else {
            Write-Host "[体积异常] $($font.Label) —— $($file.Name) 仅 $size 字节，疑似损坏" -ForegroundColor Red
        }
        $failed = $true
        continue
    }

    Write-Host "[OK] $($font.Label) —— $($file.Name)（$([math]::Round($size / 1MB, 1)) MB）" -ForegroundColor Green
}

if ($failed) {
    Show-FixSteps
    exit 1
}

Write-Host ""
Write-Host "字体校验通过（4 个字重均已落盘）。" -ForegroundColor Green
exit 0
