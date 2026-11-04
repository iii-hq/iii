import { readFileSync } from 'node:fs'
import { registerWorker } from 'iii-sdk'

// The engine URL, namespace and worker name come from the III_* environment
// that Compose hands to the guest.
const iii = registerWorker()

// Read per call: the e2e rewrites reply.txt on the host and restarts the worker,
// and the new text must come from the re-synced copy inside the VM.
iii.registerFunction('sandbox-vm-worker::hello', async () =>
  readFileSync(new URL('./reply.txt', import.meta.url), 'utf8'),
)
