#!/usr/bin/env bash
# 校验 assets/fonts/ 下的思源字体已真实落盘（不是 git-lfs 占位文件）
# 用途：拦截「克隆者未安装 git-lfs → 拿到 132 字节 pointer → 中文豆腐块」这一静默失败
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
FONT_DIR="$ROOT/assets/fonts"
MIN_BYTES=$((1024 * 1024))
LFS_POINTER_PREFIX="version https://git-lfs.github.com/spec/v1"

show_fix_steps() {
  echo ""
  echo "字体校验未通过。修复步骤："
  echo "  1. 安装 git-lfs（https://git-lfs.com）；macOS 可用 brew install git-lfs"
  echo "  2. 执行 git lfs install && git lfs pull"
  echo "  3. 重新运行：./scripts/check_fonts.sh"
  echo "详见 docs 目录下《导出引擎与排版系统_实施计划.md》的「十三、字体与 git-lfs 落地」"
}

check_font() {
  local label="$1" pattern="$2"
  local file size

  file="$(find "$FONT_DIR" -maxdepth 1 -type f -name "$pattern" 2>/dev/null | head -n 1 || true)"

  if [[ -z "$file" ]]; then
    echo "[缺失] $label —— 未找到匹配 $pattern 的文件"
    return 1
  fi

  size="$(wc -c <"$file" | tr -d '[:space:]')"

  if ((size < MIN_BYTES)); then
    if head -c 128 "$file" | grep -qF "$LFS_POINTER_PREFIX"; then
      echo "[LFS pointer] $label —— $(basename "$file") 仍是 git-lfs 占位文件（${size} 字节），未拉取真实字体"
    else
      echo "[体积异常] $label —— $(basename "$file") 仅 ${size} 字节，疑似损坏"
    fi
    return 1
  fi

  echo "[OK] $label —— $(basename "$file")（$((size / 1024 / 1024)) MB）"
}

echo "检查字体目录: $FONT_DIR"

if [[ ! -d "$FONT_DIR" ]]; then
  echo "[缺失] 字体目录不存在: $FONT_DIR"
  show_fix_steps
  exit 1
fi

failed=0
check_font "思源宋体 SC Regular" "SourceHanSerifSC-Regular.*" || failed=1
check_font "思源宋体 SC Bold" "SourceHanSerifSC-Bold.*" || failed=1
check_font "思源黑体 SC Regular" "SourceHanSansSC-Regular.*" || failed=1
check_font "思源黑体 SC Bold" "SourceHanSansSC-Bold.*" || failed=1

if ((failed)); then
  show_fix_steps
  exit 1
fi

echo ""
echo "字体校验通过（4 个字重均已落盘）。"
