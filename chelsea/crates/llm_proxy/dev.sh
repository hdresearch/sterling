#!/bin/bash
set -euo pipefail

# ─── LLM Proxy local dev setup ───────────────────────────────────────────────
#
# Usage:
#   ./dev.sh          # start proxy (creates tables on first run)
#   ./dev.sh setup    # create a dev key and add credits
#   ./dev.sh nuke     # wipe all llm_proxy tables and start fresh
#
# Requires:
#   - docker (for Postgres)
#   - cargo
#   - ANTHROPIC_API_KEY and/or OPENAI_API_KEY in env (or .env file)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
CONFIG="$SCRIPT_DIR/llm_proxy.toml"
COMPOSE="$PROJECT_DIR/pg/docker-compose.yml"
DB_URL="postgresql://postgres:opensesame@localhost:5432/vers"
ADMIN_KEY="admin-secret-change-me"
PROXY_URL="http://localhost:8090"

# Load .env if present
if [ -f "$SCRIPT_DIR/.env" ]; then
    set -a
    source "$SCRIPT_DIR/.env"
    set +a
fi

ensure_postgres() {
    if ! docker compose -f "$COMPOSE" ps --status running 2>/dev/null | grep -q pg; then
        echo "Starting Postgres..."
        docker compose -f "$COMPOSE" up -d
        echo "Waiting for Postgres..."
        for i in $(seq 1 30); do
            if psql "$DB_URL" -c "SELECT 1" &>/dev/null; then
                echo "Postgres ready."
                return
            fi
            sleep 1
        done
        echo "ERROR: Postgres did not start in 30s" >&2
        exit 1
    fi
}

cmd_start() {
    ensure_postgres

    local missing=()
    [ -z "${OPENAI_API_KEY:-}" ] && missing+=("OPENAI_API_KEY")
    [ -z "${ANTHROPIC_API_KEY:-}" ] && missing+=("ANTHROPIC_API_KEY")
    if [ ${#missing[@]} -gt 0 ]; then
        echo "WARNING: ${missing[*]} not set — those providers will return 502"
        echo "  Set them in env or create crates/llm_proxy/.env"
        echo ""
    fi

    echo "Starting llm_proxy on $PROXY_URL"
    cargo run -p llm_proxy -- "$CONFIG" --migrate
}

cmd_setup() {
    echo "Creating dev API key..."
    KEY_JSON=$(curl -sf "$PROXY_URL/admin/keys" \
        -H "Authorization: Bearer $ADMIN_KEY" \
        -H "Content-Type: application/json" \
        -d '{"name": "dev"}')

    KEY=$(echo "$KEY_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['key'])")
    KEY_ID=$(echo "$KEY_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['id'])")
    PREFIX=$(echo "$KEY_JSON" | python3 -c "import sys,json; print(json.load(sys.stdin)['key_prefix'])")

    echo "Adding credits..."
    curl -sf "$PROXY_URL/admin/keys/$KEY_ID/credits" \
        -H "Authorization: Bearer $ADMIN_KEY" \
        -H "Content-Type: application/json" \
        -d '{"amount": 10.0, "description": "dev setup"}' > /dev/null

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  Key:     $KEY"
    echo "  Prefix:  $PREFIX"
    echo "  Credits: $10.00"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Test it:"
    echo "  curl $PROXY_URL/v1/messages \\"
    echo "    -H 'Authorization: Bearer $KEY' \\"
    echo "    -H 'Content-Type: application/json' \\"
    echo "    -d '{\"model\": \"claude-haiku\", \"messages\": [{\"role\": \"user\", \"content\": \"hi\"}], \"max_tokens\": 10}'"
}

cmd_nuke() {
    ensure_postgres
    echo "Dropping all llm_proxy tables..."
    psql "$DB_URL" <<SQL
        DROP TABLE IF EXISTS llm_credit_transactions CASCADE;
        DROP TABLE IF EXISTS llm_api_keys CASCADE;
        DROP TABLE IF EXISTS llm_teams CASCADE;
        DROP TABLE IF EXISTS request_logs CASCADE;
        DROP TABLE IF EXISTS spend_logs CASCADE;
SQL
    echo "Done. Run './dev.sh' to recreate."
}

case "${1:-start}" in
    start)  cmd_start ;;
    setup)  cmd_setup ;;
    nuke)   cmd_nuke ;;
    *)
        echo "Usage: $0 [start|setup|nuke]" >&2
        exit 1
        ;;
esac
