set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

curl -s -X POST "$HTTP/links" -H 'Content-Type: application/json' \
  -d '{"url":"https://iii.dev","code":"iii"}' >/dev/null
for _ in 1 2 3 4 5; do curl -s -o /dev/null "$HTTP/s/iii"; done
sleep 3

check "engine::logs::list" 'link' "$(t engine::logs::list | head -c 4000)"
check "engine::traces::list" 'trace' "$(t engine::traces::list | head -c 4000)"

finish
