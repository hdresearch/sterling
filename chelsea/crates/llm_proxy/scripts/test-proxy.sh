#!/usr/bin/env bash
# test-proxy.sh — curl commands to exercise all llm_proxy endpoints
#
# Usage:
#   VERS_API_KEY=<uuid><secret> ./test-proxy.sh
#   VERS_API_KEY=<uuid><secret> ./test-proxy.sh http://localhost:8090

set -euo pipefail

BASE="${1:-https://tokens.vers.sh}"
ADMIN="${ADMIN_API_KEY:-6e4d81e5597985212ca9381a082f280e1864c0389aab9311cdc53efac0da3978}"

if [[ -z "${VERS_API_KEY:-}" ]]; then
  echo "Error: VERS_API_KEY must be set (your Vers platform API key)"
  echo "Usage: VERS_API_KEY=<uuid><secret> $0 [base_url]"
  exit 1
fi

echo "=== 1. Health ==="
curl -s $BASE/health | jq .

echo ""
echo "=== 2. Models ==="
curl -s $BASE/v1/models | jq '.data[].id'

echo ""
echo "=== 3. Exchange Vers key for LLM key ==="
EXCHANGE=$(curl -s $BASE/v1/keys/exchange \
  -H "Content-Type: application/json" \
  -d "{\"vers_api_key\":\"$VERS_API_KEY\",\"name\":\"test-$(date +%s)\"}")
echo "$EXCHANGE" | jq .
KEY=$(echo "$EXCHANGE" | jq -r .key)
KEY_ID=$(echo "$EXCHANGE" | jq -r .id)
TEAM_ID=$(echo "$EXCHANGE" | jq -r .team_id)

if [[ "$KEY" == "null" || -z "$KEY" ]]; then
  echo "❌ Key exchange failed, cannot continue"
  exit 1
fi

echo ""
echo "=== 4. List Keys (admin) ==="
curl -s $BASE/admin/keys -H "Authorization: Bearer $ADMIN" | jq '.[].name'

echo ""
echo "=== 5. Add Credits ==="
curl -s $BASE/admin/keys/$KEY_ID/credits \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ADMIN" \
  -d '{"amount":5,"description":"smoke test"}' | jq .

echo ""
echo "=== 6. Credit History ==="
curl -s $BASE/admin/keys/$KEY_ID/credits/history \
  -H "Authorization: Bearer $ADMIN" | jq .

echo ""
echo "=== 7. Anthropic /v1/messages ==="
curl -s $BASE/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-haiku","max_tokens":16,"messages":[{"role":"user","content":"Say pong"}]}' | jq .

echo ""
echo "=== 8. OpenAI /v1/chat/completions ==="
curl -s $BASE/v1/chat/completions \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $KEY" \
  -d '{"model":"claude-haiku","max_tokens":16,"messages":[{"role":"user","content":"Say pong"}]}' | jq .

echo ""
echo "=== 9. Spend Tracking ==="
sleep 1
curl -s "$BASE/admin/spend?api_key_id=$KEY_ID" \
  -H "Authorization: Bearer $ADMIN" | jq .

echo ""
echo "=== 10. Spend by Model ==="
curl -s $BASE/admin/spend/models \
  -H "Authorization: Bearer $ADMIN" | jq .

echo ""
echo "=== 11. Update Budget ==="
curl -s $BASE/admin/keys/$KEY_ID/budget \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer $ADMIN" \
  -d '{"max_budget":50}' | jq .

echo ""
echo "=== 12. Bad Key (expect 401) ==="
curl -s -w "\nHTTP %{http_code}\n" $BASE/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: sk-vers-bad" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-haiku","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'

echo ""
echo "=== 13. Revoke Key ==="
curl -s -X DELETE $BASE/admin/keys/$KEY_ID \
  -H "Authorization: Bearer $ADMIN" | jq .

echo ""
echo "=== 14. Revoked Key (expect 401/403) ==="
curl -s -w "\nHTTP %{http_code}\n" $BASE/v1/messages \
  -H "Content-Type: application/json" \
  -H "x-api-key: $KEY" \
  -H "anthropic-version: 2023-06-01" \
  -d '{"model":"claude-haiku","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}'
