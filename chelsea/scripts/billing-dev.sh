#!/bin/bash
#
# Local billing development environment.
#
# Starts everything needed to test Stripe billing locally:
#   1. Postgres (docker) with all migrations (chelsea + vers-landing)
#   2. Webhook test server (receives Stripe webhooks)
#   3. Stripe CLI listener (forwards webhooks from Stripe to local server)
#
# Prerequisites:
#   - docker
#   - dbmate (brew install dbmate)
#   - stripe CLI (brew install stripe/stripe-cli/stripe), logged in
#   - Rust toolchain
#
# Usage:
#   ./scripts/billing-dev.sh           # start everything
#   ./scripts/billing-dev.sh db        # just start postgres + run migrations
#   ./scripts/billing-dev.sh trigger   # send test webhook events
#   ./scripts/billing-dev.sh stop      # stop everything
#
# Environment variables (optional):
#   STRIPE_SECRET_KEY   — defaults to the test key from vers-landing/.env
#   VERS_LANDING_DIR    — path to vers-landing repo (for org_subscriptions migration)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PID_DIR="${PROJECT_DIR}/.billing-dev"

POSTGRES_PASSWORD=opensesame
POSTGRES_USER=postgres
POSTGRES_DB=vers
DATABASE_URL="postgresql://${POSTGRES_USER}:${POSTGRES_PASSWORD}@127.0.0.1:5432/${POSTGRES_DB}?sslmode=disable"

WEBHOOK_PORT=8080
WEBHOOK_URL="http://localhost:${WEBHOOK_PORT}/api/v1/billing/webhooks/stripe"
LLM_PROXY_PORT=8090

# Default to the test key from vers-landing
STRIPE_SECRET_KEY="${STRIPE_SECRET_KEY:-sk_test_51TC1LOHkckoN2oROVhUEYVyedKk03ttcEEidNXSXxVJQzjSFQdW3U2CNqYB6fvqMWXl8pIb2FJa0cOvVSoFgTfU000mIxmIPbM}"

# Try to find vers-landing for org_subscriptions migrations
VERS_LANDING_DIR="${VERS_LANDING_DIR:-${PROJECT_DIR}/../vers-landing}"

# ─── Helpers ──────────────────────────────────────────────────────────

check_deps() {
    local missing=()
    command -v docker >/dev/null 2>&1 || missing+=("docker")
    command -v dbmate >/dev/null 2>&1 || missing+=("dbmate")
    command -v stripe >/dev/null 2>&1 || missing+=("stripe CLI")
    command -v cargo >/dev/null 2>&1  || missing+=("cargo")

    if [ ${#missing[@]} -gt 0 ]; then
        echo "❌ Missing dependencies: ${missing[*]}"
        echo ""
        echo "Install with:"
        echo "  brew install dbmate"
        echo "  brew install stripe/stripe-cli/stripe"
        exit 1
    fi
}

ensure_pid_dir() {
    mkdir -p "$PID_DIR"
}

# ─── Database ─────────────────────────────────────────────────────────

start_db() {
    echo "🐘 Starting Postgres..."
    cd "${PROJECT_DIR}/pg"
    docker compose up -d 2>/dev/null || docker-compose up -d 2>/dev/null

    # Wait for postgres to be ready (handles both fresh init and restarts)
    echo "   Waiting for Postgres to accept connections..."
    local attempts=0
    while [ $attempts -lt 30 ]; do
        if PGPASSWORD="$POSTGRES_PASSWORD" psql -h 127.0.0.1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c "SELECT 1" >/dev/null 2>&1; then
            break
        fi
        # If auth fails, the volume may have a stale password — recreate it
        if PGPASSWORD="$POSTGRES_PASSWORD" psql -h 127.0.0.1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c "SELECT 1" 2>&1 | grep -q "password authentication failed"; then
            echo "   ⚠️  Stale volume detected (wrong password). Recreating..."
            docker compose down -v 2>/dev/null || docker-compose down -v 2>/dev/null
            docker compose up -d 2>/dev/null || docker-compose up -d 2>/dev/null
            attempts=0
        fi
        sleep 1
        attempts=$((attempts + 1))
    done

    if ! PGPASSWORD="$POSTGRES_PASSWORD" psql -h 127.0.0.1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -c "SELECT 1" >/dev/null 2>&1; then
        echo "❌ Postgres failed to start after 30s"
        exit 1
    fi

    echo "📦 Running chelsea migrations..."
    dbmate --url "$DATABASE_URL" \
           --migrations-dir ./migrations \
           --no-dump-schema \
           up --strict

    # Apply vers-landing billing tables directly.
    # We can't use dbmate because both repos share schema_migrations
    # and have colliding version timestamps.
    echo "📦 Applying billing tables from vers-landing..."
    PGPASSWORD="$POSTGRES_PASSWORD" psql -h 127.0.0.1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -q <<'BILLING_SQL'
    -- In production, vers-landing creates tables in the vers_landing schema.
    -- Our billing queries reference vers_landing.org_subscriptions.
    CREATE SCHEMA IF NOT EXISTS vers_landing;

    -- org_subscriptions (from vers-landing 20260127000001 + later migrations)
    CREATE TABLE IF NOT EXISTS vers_landing.org_subscriptions (
        org_id UUID PRIMARY KEY REFERENCES public.organizations(org_id) ON DELETE CASCADE,
        billing_contact_id UUID NOT NULL REFERENCES public.users(user_id),
        tier VARCHAR(50) NOT NULL DEFAULT 'none',
        status VARCHAR(50) NOT NULL DEFAULT 'none',
        flowglad_customer_id VARCHAR(255),
        flowglad_subscription_id VARCHAR(255),
        flowglad_product_id VARCHAR(255),
        flowglad_price_id VARCHAR(255),
        billing_period_start TIMESTAMPTZ,
        billing_period_end TIMESTAMPTZ,
        is_free_plan BOOLEAN NOT NULL DEFAULT FALSE,
        created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
        billing_provider TEXT NOT NULL DEFAULT 'stripe',
        auto_topup_enabled BOOLEAN NOT NULL DEFAULT FALSE,
        auto_topup_threshold_cents INTEGER NOT NULL DEFAULT 500,
        auto_topup_amount_cents INTEGER NOT NULL DEFAULT 5000,
        pending_adjustment VARCHAR(50),
        CONSTRAINT valid_org_tier CHECK (tier IN ('none', 'free', 'starter', 'pro', 'team', 'enterprise')),
        CONSTRAINT valid_org_status CHECK (status IN ('none', 'active', 'trialing', 'past_due', 'cancelled', 'canceled', 'cancellation_scheduled'))
    );
    CREATE INDEX IF NOT EXISTS idx_org_subscriptions_billing_contact ON vers_landing.org_subscriptions(billing_contact_id);
    CREATE INDEX IF NOT EXISTS idx_org_subscriptions_tier ON vers_landing.org_subscriptions(tier);
    CREATE INDEX IF NOT EXISTS idx_org_subscriptions_status ON vers_landing.org_subscriptions(status);
    CREATE INDEX IF NOT EXISTS idx_org_subscriptions_flowglad_customer ON vers_landing.org_subscriptions(flowglad_customer_id) WHERE flowglad_customer_id IS NOT NULL;

    -- Seed: create an org subscription linked to a Stripe test customer
    INSERT INTO vers_landing.org_subscriptions (
        org_id, billing_contact_id, tier, status, billing_provider,
        flowglad_customer_id, is_free_plan
    ) VALUES (
        '2fbd38fd-aaed-4fae-9f9a-f75ae3ef313d',  -- test_user org from seed
        '9e92f9ad-3c1e-4e70-b5c4-e60de0d646e9',  -- test@vers.sh user from seed
        'starter', 'active', 'stripe',
        'cus_UAUqvLpc4aNh9V',                      -- Stripe test customer
        false
    ) ON CONFLICT (org_id) DO NOTHING;

    -- Seed: create an llm_team for the test org (if not exists)
    INSERT INTO llm_teams (id, org_id, name)
    VALUES (
        'a1b2c3d4-e5f6-7890-abcd-ef0123456789',
        '2fbd38fd-aaed-4fae-9f9a-f75ae3ef313d',
        'Default'
    ) ON CONFLICT (org_id) DO NOTHING;
BILLING_SQL

    echo "✅ Database ready at ${DATABASE_URL}"
}

stop_db() {
    echo "🐘 Stopping Postgres..."
    cd "${PROJECT_DIR}/pg"
    docker compose down 2>/dev/null || docker-compose down 2>/dev/null || true
}

# ─── Webhook Server ──────────────────────────────────────────────────

start_webhook_server() {
    local webhook_secret="$1"

    echo "🔨 Building webhook server..."
    cd "$PROJECT_DIR"
    cargo build -p billing --example test_webhook_server 2>&1 | tail -1

    echo "🚀 Starting webhook server on port ${WEBHOOK_PORT}..."
    STRIPE_SECRET_KEY="$STRIPE_SECRET_KEY" \
    STRIPE_WEBHOOK_SECRET="$webhook_secret" \
    DATABASE_URL="$DATABASE_URL" \
    cargo run -p billing --example test_webhook_server \
        > "${PID_DIR}/webhook-server.log" 2>&1 &

    echo $! > "${PID_DIR}/webhook-server.pid"
    sleep 2

    if kill -0 "$(cat "${PID_DIR}/webhook-server.pid")" 2>/dev/null; then
        echo "✅ Webhook server running (PID $(cat "${PID_DIR}/webhook-server.pid"))"
    else
        echo "❌ Webhook server failed to start. Check ${PID_DIR}/webhook-server.log"
        cat "${PID_DIR}/webhook-server.log"
        exit 1
    fi
}

stop_webhook_server() {
    if [ -f "${PID_DIR}/webhook-server.pid" ]; then
        local pid
        pid=$(cat "${PID_DIR}/webhook-server.pid")
        if kill -0 "$pid" 2>/dev/null; then
            echo "🛑 Stopping webhook server (PID $pid)..."
            kill "$pid" 2>/dev/null || true
        fi
        rm -f "${PID_DIR}/webhook-server.pid"
    fi
}

# ─── LLM Proxy ────────────────────────────────────────────────────

start_llm_proxy() {
    local webhook_secret="$1"

    echo "🔨 Building llm_proxy..."
    cd "$PROJECT_DIR"
    cargo build -p llm_proxy 2>&1 | tail -1

    # Generate a dev config with Stripe enabled
    cat > "${PID_DIR}/llm_proxy.toml" <<TOML
[server]
host = "0.0.0.0"
port = ${LLM_PROXY_PORT}
admin_api_key = "admin-dev-key"

[database]
url = "${DATABASE_URL}"

[stripe]
secret_key = "${STRIPE_SECRET_KEY}"
meter_event_name = "llm_spend"
balance_poll_interval_secs = 30
meter_flush_interval_secs = 5

[providers.anthropic]
type = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"

[providers.openai]
type = "openai"
api_key_env = "OPENAI_API_KEY"

[models.claude-sonnet]
routing = ["anthropic"]
model_name = "claude-sonnet-4-20250514"

[models.claude-haiku]
routing = ["anthropic"]
model_name = "claude-haiku-4-5-20251001"

[models.gpt-4o]
routing = ["openai"]

[models.gpt-4o-mini]
routing = ["openai"]
TOML

    echo "🚀 Starting llm_proxy on port ${LLM_PROXY_PORT}..."
    cargo run -p llm_proxy -- "${PID_DIR}/llm_proxy.toml" --migrate \
        > "${PID_DIR}/llm-proxy.log" 2>&1 &
    echo $! > "${PID_DIR}/llm-proxy.pid"
    sleep 3

    if kill -0 "$(cat "${PID_DIR}/llm-proxy.pid")" 2>/dev/null; then
        echo "✅ LLM proxy running on port ${LLM_PROXY_PORT} (PID $(cat "${PID_DIR}/llm-proxy.pid"))"
    else
        echo "❌ LLM proxy failed to start. Check ${PID_DIR}/llm-proxy.log"
        tail -20 "${PID_DIR}/llm-proxy.log"
        exit 1
    fi

    # Create a dev API key (linked to seeded user + team)
    echo ""
    echo "🔑 Creating dev API key..."
    local key_response
    key_response=$(curl -s -X POST "http://localhost:${LLM_PROXY_PORT}/admin/keys" \
        -H "Authorization: Bearer admin-dev-key" \
        -H "Content-Type: application/json" \
        -d '{
            "name": "billing-dev-test",
            "user_id": "9e92f9ad-3c1e-4e70-b5c4-e60de0d646e9",
            "team_id": "a1b2c3d4-e5f6-7890-abcd-ef0123456789"
        }')

    local api_key
    api_key=$(echo "$key_response" | python3 -c "import json,sys; print(json.load(sys.stdin).get('key',''))" 2>/dev/null || true)

    if [ -n "$api_key" ]; then
        echo "$api_key" > "${PID_DIR}/api-key"
        echo "✅ API key created: ${api_key}"
    else
        echo "⚠️  Could not create API key. Response: ${key_response}"
        echo "   Create one manually:"
        echo "   curl -X POST http://localhost:${LLM_PROXY_PORT}/admin/keys -H 'Authorization: Bearer admin-dev-key' -H 'Content-Type: application/json' -d '{\"name\": \"test\"}'"
    fi
}

stop_llm_proxy() {
    if [ -f "${PID_DIR}/llm-proxy.pid" ]; then
        local pid
        pid=$(cat "${PID_DIR}/llm-proxy.pid")
        if kill -0 "$pid" 2>/dev/null; then
            echo "🛑 Stopping llm_proxy (PID $pid)..."
            kill "$pid" 2>/dev/null || true
        fi
        rm -f "${PID_DIR}/llm-proxy.pid"
    fi
}

# ─── Stripe Listener ─────────────────────────────────────────────────

start_stripe_listener() {
    echo "📡 Starting Stripe listener → ${WEBHOOK_URL}..."

    # Start stripe listen and capture the webhook secret from its output
    stripe listen --forward-to "$WEBHOOK_URL" \
        > "${PID_DIR}/stripe-listen.log" 2>&1 &
    echo $! > "${PID_DIR}/stripe-listen.pid"

    # Wait for the webhook secret to appear in output
    echo "   Waiting for webhook secret..."
    local attempts=0
    local webhook_secret=""
    while [ $attempts -lt 15 ]; do
        webhook_secret=$(grep -o 'whsec_[a-zA-Z0-9]*' "${PID_DIR}/stripe-listen.log" 2>/dev/null | head -1 || true)
        if [ -n "$webhook_secret" ]; then
            break
        fi
        sleep 1
        attempts=$((attempts + 1))
    done

    if [ -z "$webhook_secret" ]; then
        echo "❌ Failed to get webhook secret from stripe listen"
        echo "   Is the Stripe CLI logged in? Try: stripe login"
        cat "${PID_DIR}/stripe-listen.log"
        stop_stripe_listener
        exit 1
    fi

    echo "$webhook_secret" > "${PID_DIR}/webhook-secret"
    echo "✅ Stripe listener running (secret: ${webhook_secret})"
    echo "$webhook_secret"
}

stop_stripe_listener() {
    if [ -f "${PID_DIR}/stripe-listen.pid" ]; then
        local pid
        pid=$(cat "${PID_DIR}/stripe-listen.pid")
        if kill -0 "$pid" 2>/dev/null; then
            echo "🛑 Stopping Stripe listener (PID $pid)..."
            kill "$pid" 2>/dev/null || true
        fi
        rm -f "${PID_DIR}/stripe-listen.pid"
    fi
}

# ─── Commands ─────────────────────────────────────────────────────────

cmd_start() {
    check_deps
    ensure_pid_dir

    echo ""
    echo "═══════════════════════════════════════════"
    echo "  Billing Dev Environment"
    echo "═══════════════════════════════════════════"
    echo ""

    start_db
    echo ""

    local webhook_secret
    webhook_secret=$(start_stripe_listener)
    echo ""

    start_webhook_server "$webhook_secret"
    echo ""

    start_llm_proxy "$webhook_secret"

    # Read back the API key if it was created
    local api_key_display=""
    if [ -f "${PID_DIR}/api-key" ]; then
        api_key_display=$(cat "${PID_DIR}/api-key")
    fi

    echo ""
    echo "═══════════════════════════════════════════"
    echo "  ✅ Ready!"
    echo ""
    echo "  LLM Proxy:    http://localhost:${LLM_PROXY_PORT}"
    echo "  Webhook URL:  ${WEBHOOK_URL}"
    echo "  Database:     ${DATABASE_URL}"
    echo "  Stripe key:   ${STRIPE_SECRET_KEY:0:20}..."
    if [ -n "$api_key_display" ]; then
    echo "  API key:      ${api_key_display}"
    fi
    echo ""
    echo "  Test LLM request:"
    if [ -n "$api_key_display" ]; then
    echo "    curl http://localhost:${LLM_PROXY_PORT}/v1/chat/completions \\"
    echo "      -H 'Authorization: Bearer ${api_key_display}' \\"
    echo "      -H 'Content-Type: application/json' \\"
    echo "      -d '{\"model\": \"claude-haiku\", \"messages\": [{\"role\": \"user\", \"content\": \"hello\"}]}'"
    else
    echo "    (create an API key first — see above)"
    fi
    echo ""
    echo "  Test webhooks:"
    echo "    stripe trigger checkout.session.completed"
    echo "    stripe trigger customer.subscription.created"
    echo "    stripe trigger invoice.paid"
    echo ""
    echo "  Logs:"
    echo "    tail -f ${PID_DIR}/llm-proxy.log"
    echo "    tail -f ${PID_DIR}/webhook-server.log"
    echo "    tail -f ${PID_DIR}/stripe-listen.log"
    echo ""
    echo "  Stop with:"
    echo "    ./scripts/billing-dev.sh stop"
    echo "═══════════════════════════════════════════"
}

cmd_db() {
    check_deps
    start_db
}

cmd_trigger() {
    echo "Triggering test events..."
    echo ""
    echo "→ checkout.session.completed"
    stripe trigger checkout.session.completed 2>&1 | tail -1
    echo "→ customer.subscription.created"
    stripe trigger customer.subscription.created 2>&1 | tail -1
    echo "→ invoice.paid"
    stripe trigger invoice.paid 2>&1 | tail -1
    echo ""
    echo "Done. Check webhook server logs:"
    echo "  tail -f ${PID_DIR}/webhook-server.log"
}

cmd_stop() {
    ensure_pid_dir
    stop_llm_proxy
    stop_webhook_server
    stop_stripe_listener
    echo "✅ Billing dev processes stopped."
    echo "   (Postgres left running. Stop with: ./scripts/billing-dev.sh stop-all)"
}

cmd_stop_all() {
    cmd_stop
    stop_db
    echo "✅ Everything stopped."
}

cmd_logs() {
    if [ -f "${PID_DIR}/webhook-server.log" ]; then
        tail -f "${PID_DIR}/webhook-server.log"
    else
        echo "No logs found. Is the billing dev environment running?"
    fi
}

# ─── Main ─────────────────────────────────────────────────────────────

case "${1:-start}" in
    start)    cmd_start ;;
    db)       cmd_db ;;
    trigger)  cmd_trigger ;;
    stop)     cmd_stop ;;
    stop-all) cmd_stop_all ;;
    logs)     cmd_logs ;;
    *)
        echo "Usage: $0 {start|db|trigger|stop|stop-all|logs}"
        exit 1
        ;;
esac
