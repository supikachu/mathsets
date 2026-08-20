#!/usr/bin/env bash
# 创建独立测试库并执行迁移（与 .env 中 DATABASE_URL_TEST 对应）
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ -f .env ]]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi

: "${DATABASE_URL:?请设置 DATABASE_URL 或在 .env 中配置}"

TEST_URL="${DATABASE_URL_TEST:-${DATABASE_URL%/*}/mathset_test}"
DB_NAME="${TEST_URL##*/}"
DB_NAME="${DB_NAME%%\?*}"
ADMIN_URL="${DATABASE_URL%/*}/postgres"

echo "创建测试库 ${DB_NAME}（若已存在则跳过）..."
if ! psql "$ADMIN_URL" -tAc "SELECT 1 FROM pg_database WHERE datname = '${DB_NAME}'" | grep -q 1; then
  psql "$ADMIN_URL" -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"${DB_NAME}\""
fi

echo "对测试库执行迁移: ${TEST_URL}"
DATABASE_URL="$TEST_URL" sqlx migrate run

echo "完成。请在 .env 中设置: DATABASE_URL_TEST=${TEST_URL}"
