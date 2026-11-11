import { existsSync, readFileSync } from 'node:fs'
import { registerWorker } from 'iii-sdk'

// The engine URL, namespace and worker name come from the III_* environment
// that Compose hands to the guest.
const iii = registerWorker()

// Read per call: the e2e rewrites reply.txt on the host and restarts the worker,
// and the new text must come from the re-synced copy inside the VM.
iii.registerFunction('sandbox-vm-worker::hello', async () =>
  readFileSync(new URL('./reply.txt', import.meta.url), 'utf8'),
)

// Whether `.iii` is in the worker's directory, `/workspace` in the guest: the
// e2e puts `.iii/build/` and `.iii/workers/` there on the host, and no copy or
// sync may carry them in.
iii.registerFunction('sandbox-vm-worker::sees-iii', async () => ({
  iii: existsSync(new URL('./.iii', import.meta.url)),
}))
