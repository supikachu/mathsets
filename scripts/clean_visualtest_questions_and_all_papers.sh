#!/usr/bin/env bash
# =============================================================================
# 清理脚本：删除用户 visualtest 最近一周添加的题目，并清空数据库中所有试卷（papers）
#
# 使用方式：
#   ./scripts/clean_visualtest_questions_and_all_papers.sh
#   DATABASE_URL="postgres://postgres:postgres@localhost:5432/mathset" ./scripts/clean_visualtest_questions_and_all_papers.sh
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SQL_FILE="${SCRIPT_DIR}/clean_visualtest_questions_and_all_papers.sql"
DB_NAME="${DB_NAME:-mathset}"
DB_USER="${DB_USER:-postgres}"
DB_HOST="${DB_HOST:-127.0.0.1}"
DB_PORT="${DB_PORT:-5432}"

echo "============================================================"
echo " 🧹 MathSet 数据清理脚本"
echo " 目标: 清理用户 [visualtest] 最近 7 天题目 + 清空全部试卷"
echo " 数据库: ${DB_NAME} @ ${DB_HOST}:${DB_PORT} (用户: ${DB_USER})"
echo "============================================================"

if [ -n "${DATABASE_URL:-}" ]; then
  psql "${DATABASE_URL}" -f "${SQL_FILE}"
else
  psql -h "${DB_HOST}" -p "${DB_PORT}" -U "${DB_USER}" -d "${DB_NAME}" -f "${SQL_FILE}"
fi

echo "✨ 清理完成！"
