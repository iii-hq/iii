#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${1:-http://localhost:3111}"
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
NC='\033[0m'

header() { echo -e "\n${CYAN}━━━ $1 ━━━${NC}"; }
label()  { echo -e "${YELLOW}▶ $1${NC}"; }

header "Basic Queue — Enqueue"
label "POST /queue/enqueue"
curl -s -X POST "$BASE_URL/queue/enqueue" \
  -H 'Content-Type: application/json' \
  -d '{"task":"hello","priority":1}' | jq .

header "Basic Queue — Enqueue with Error Handling"
label "POST /queue/enqueue-safe"
curl -s -X POST "$BASE_URL/queue/enqueue-safe" \
  -H 'Content-Type: application/json' \
  -d '{"task":"safe-job"}' | jq .

header "Basic Queue — Fire and Forget (Void)"
label "POST /queue/fire-and-forget"
curl -s -X POST "$BASE_URL/queue/fire-and-forget" \
  -H 'Content-Type: application/json' \
  -d '{"event":"page_viewed","page":"/home"}' | jq .

header "E-Commerce — Create Order (payment FIFO + email + analytics void)"
label "POST /orders"
curl -s -X POST "$BASE_URL/orders" \
  -H 'Content-Type: application/json' \
  -d '{"email":"customer@example.com","total":149.99}' | jq .

header "Bulk Email — Launch Campaign"
label "POST /campaigns/launch"
curl -s -X POST "$BASE_URL/campaigns/launch" \
  -H 'Content-Type: application/json' \
  -d '{
    "subject":"Welcome to III",
    "body":"Thanks for signing up!",
    "recipients":[
      {"email":"alice@example.com"},
      {"email":"bob@example.com"},
      {"email":"carol@example.com"}
    ]
  }' | jq .

header "Financial Ledger — Deposit (FIFO)"
label "POST /transactions (deposit 500)"
curl -s -X POST "$BASE_URL/transactions" \
  -H 'Content-Type: application/json' \
  -d '{"account_id":"acc-001","type":"deposit","amount":500}' | jq .

label "POST /transactions (deposit 250)"
curl -s -X POST "$BASE_URL/transactions" \
  -H 'Content-Type: application/json' \
  -d '{"account_id":"acc-001","type":"deposit","amount":250}' | jq .

label "POST /transactions (withdraw 100)"
curl -s -X POST "$BASE_URL/transactions" \
  -H 'Content-Type: application/json' \
  -d '{"account_id":"acc-001","type":"withdraw","amount":100}' | jq .

sleep 1
label "GET /accounts/acc-001/balance"
curl -s "$BASE_URL/accounts/acc-001/balance" | jq .

header "DLQ Demo — Job That Will Fail (lands in DLQ after 2 retries)"
label "POST /dlq/enqueue (will fail)"
curl -s -X POST "$BASE_URL/dlq/enqueue" \
  -H 'Content-Type: application/json' \
  -d '{"message":"this will fail and go to DLQ"}' | jq .

header "DLQ Demo — Job That Will Succeed"
label "POST /dlq/enqueue (will succeed)"
curl -s -X POST "$BASE_URL/dlq/enqueue" \
  -H 'Content-Type: application/json' \
  -d '{"succeed":true,"message":"this will process fine"}' | jq .

echo -e "\n${GREEN}✔ All queue tests sent. Check worker logs for processing output.${NC}"
echo -e "${GREEN}  DLQ messages will appear in ~2s at: Console → Queues → dlq-demo → Dead Letters${NC}"
