#!/usr/bin/env bash
set -euo pipefail

BASE_URL="${1:-http://localhost:3111}"
TOTAL="${2:-1000}"
CONCURRENCY="${3:-50}"
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m'

echo -e "${CYAN}━━━ DLQ Stress Test ━━━${NC}"
echo -e "${YELLOW}Target:      ${BASE_URL}/dlq/enqueue${NC}"
echo -e "${YELLOW}Total:       ${TOTAL} messages${NC}"
echo -e "${YELLOW}Concurrency: ${CONCURRENCY} parallel requests${NC}"
echo ""

SUCCESS=0
FAIL=0
ERRORS=0
START=$(date +%s)

send_message() {
  local i=$1
  local response
  response=$(curl -s -o /dev/null -w "%{http_code}" -X POST "$BASE_URL/dlq/enqueue" \
    -H 'Content-Type: application/json' \
    -d "{\"message\":\"stress-test-msg-${i}\",\"index\":${i}}" 2>/dev/null) || true

  if [[ "$response" == "200" || "$response" == "201" || "$response" == "202" ]]; then
    echo "OK"
  elif [[ -z "$response" ]]; then
    echo "ERR"
  else
    echo "FAIL:${response}"
  fi
}

export -f send_message
export BASE_URL

echo -e "${CYAN}Sending ${TOTAL} messages (all will fail and land in DLQ)...${NC}"

RESULTS=$(seq 1 "$TOTAL" | xargs -P "$CONCURRENCY" -I {} bash -c 'send_message "$@"' _ {})

while IFS= read -r line; do
  case "$line" in
    OK)      ((SUCCESS++)) ;;
    ERR)     ((ERRORS++)) ;;
    FAIL:*)  ((FAIL++)) ;;
  esac
done <<< "$RESULTS"

END=$(date +%s)
ELAPSED=$((END - START))

echo ""
echo -e "${CYAN}━━━ Results ━━━${NC}"
echo -e "${GREEN}Accepted:    ${SUCCESS}${NC}"
echo -e "${RED}HTTP errors: ${FAIL}${NC}"
echo -e "${RED}Net errors:  ${ERRORS}${NC}"
echo -e "${YELLOW}Duration:    ${ELAPSED}s${NC}"
echo -e "${YELLOW}Throughput:  ~$(( TOTAL / (ELAPSED > 0 ? ELAPSED : 1) )) req/s${NC}"
echo ""
echo -e "${GREEN}✔ Stress test complete. Check DLQ for failed messages.${NC}"
