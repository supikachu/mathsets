#!/usr/bin/env bash
# =============================================================================
# Delete specific records by ID from a specific table in PostgreSQL
# =============================================================================

set -e

TABLE=""
IDS=()
DB_NAME="mathset"
DB_USER="postgres"
DB_HOST="127.0.0.1"
DB_PORT="5432"
DRY_RUN=false
FORCE=false

usage() {
    echo "Usage: $0 --table <table_name> --id <uuid1,uuid2...> [options]"
    echo ""
    echo "Options:"
    echo "  -t, --table       Target table name (e.g. questions, papers, users, etc.)"
    echo "  -i, --id          Target ID (UUID), comma-separated IDs, or multiple --id flags"
    echo "  -d, --database    Database name (default: mathset)"
    echo "  -u, --username    Database user (default: postgres)"
    echo "  -h, --host        Database host (default: 127.0.0.1)"
    echo "  -p, --port        Database port (default: 5432)"
    echo "  --dry-run         Preview mode without modifying database"
    echo "  -f, --force       Skip confirmation prompt"
    echo "  --help            Show this help message"
    exit 1
}

while [[ "$#" -gt 0 ]]; do
    case $1 in
        -t|--table) TABLE="$2"; shift ;;
        -i|--id)
            IFS=',' read -ra ADDR <<< "$2"
            for id in "${ADDR[@]}"; do
                IDS+=("$(echo "$id" | xargs)")
            done
            shift
            ;;
        -d|--database) DB_NAME="$2"; shift ;;
        -u|--username) DB_USER="$2"; shift ;;
        -h|--host) DB_HOST="$2"; shift ;;
        -p|--port) DB_PORT="$2"; shift ;;
        --dry-run) DRY_RUN=true ;;
        -f|--force) FORCE=true ;;
        --help) usage ;;
        *) echo "Unknown parameter: $1"; usage ;;
    esac
    shift
done

if [ -z "$TABLE" ] || [ "${#IDS[@]}" -eq 0 ]; then
    echo "Error: Missing required --table or --id parameter."
    usage
fi

echo "============================================================"
echo " [MathSet Database Record Deletion Script]"
echo " Target Table : $TABLE"
echo " Target IDs   : ${#IDS[@]} record(s)"
echo " Database     : $DB_NAME @ $DB_HOST:$DB_PORT (User: $DB_USER)"
echo " Mode         : $([ "$DRY_RUN" = true ] && echo 'DRY-RUN (Preview Only)' || echo 'EXECUTE')"
echo "============================================================"

# Format UUID array
PG_ARRAY="ARRAY["
for i in "${!IDS[@]}"; do
    if [ "$i" -gt 0 ]; then
        PG_ARRAY+=", "
    fi
    PG_ARRAY+="'${IDS[$i]}'::uuid"
done
PG_ARRAY+="]"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SQL_FILE="$SCRIPT_DIR/delete_records.sql"

if [ -f "$SQL_FILE" ]; then
    psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -q -v ON_ERROR_STOP=1 -f "$SQL_FILE" > /dev/null 2>&1 || true
fi

echo ""
echo "Querying affected records and relations..."
PREVIEW_SQL="SELECT step_name, affected_table, deleted_count FROM mathset_delete_records('$TABLE', $PG_ARRAY, true);"
psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -v ON_ERROR_STOP=1 -c "$PREVIEW_SQL"

if [ "$DRY_RUN" = true ]; then
    echo ""
    echo "[DryRun Mode] Preview completed. No data was modified in the database."
    exit 0
fi

if [ "$FORCE" != true ]; then
    read -p "Are you sure you want to permanently delete the above record(s) from table [$TABLE]? (y/N): " CONFIRM
    if [[ ! "$CONFIRM" =~ ^[Yy]$ ]]; then
        echo "Operation cancelled."
        exit 0
    fi
fi

echo ""
echo "Executing deletion transaction..."
EXEC_SQL="BEGIN; SELECT step_name, affected_table, deleted_count FROM mathset_delete_records('$TABLE', $PG_ARRAY, false); COMMIT;"
psql -h "$DB_HOST" -p "$DB_PORT" -U "$DB_USER" -d "$DB_NAME" -v ON_ERROR_STOP=1 -c "$EXEC_SQL"

echo ""
echo "Deletion completed successfully!"
