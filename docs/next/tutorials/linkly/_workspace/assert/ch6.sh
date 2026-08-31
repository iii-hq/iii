set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

check "csv imported over a channel" 'imported: 2' \
  "$(cd "$PROJECT/channel-client" && node import-links.js 2>&1 | tail -3)"
sleep 2
check "mylink resolves" 'https://iii.dev' "$(t link::resolve code=mylink)"
check "mydocslink resolves" 'https://iii.dev/docs' "$(t link::resolve code=mydocslink)"

finish
