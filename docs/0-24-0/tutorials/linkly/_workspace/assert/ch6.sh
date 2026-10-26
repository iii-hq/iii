set -uo pipefail
source "$(dirname "${BASH_SOURCE[0]}")/lib.sh"

# The bulk-importer runs in a VM, so its import_csv function can register a few
# seconds after the port is open. A reader waits for the "ready" log; the test
# retries the client until the function answers instead of the first call racing
# it. A failed call imports nothing, so a retry never double-imports.
import_out=""
for _ in $(seq 1 30); do
  import_out="$(cd "$PROJECT/channel-client" && node import-links.js 2>&1 | tail -3)"
  [[ "$import_out" == *"imported"* ]] && break
  sleep 1
done
check "csv imported over a channel" 'imported: 2' "$import_out"
sleep 2
check "mylink resolves" 'https://iii.dev' "$(t link::resolve code=mylink)"
check "mydocslink resolves" 'https://iii.dev/docs' "$(t link::resolve code=mydocslink)"

finish
