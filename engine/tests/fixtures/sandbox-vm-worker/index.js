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

// The build's copy into /workspace lists what it extracted here, one `./<path>`
// line per entry, and the file ships in the image.
const BUILD_MANIFEST = '/var/lib/iii/build.manifest'

// The e2e puts `.iii/build/` and `.iii/workers/` in the worker's directory on
// the host, and no copy or sync may carry them in. `iii`: whether `.iii` is in
// the worker's directory, `/workspace` in the guest. `built`: whether the
// build's copy took a `.iii` into the image; a boot deletes `/workspace/.iii`,
// so only the manifest still shows it. `null` without a manifest, or with one
// that does not list this file, which fails the check instead of passing it.
iii.registerFunction('sandbox-vm-worker::sees-iii', async () => {
  let lines = []
  try {
    lines = readFileSync(BUILD_MANIFEST, 'utf8').split('\n')
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
  }
  return {
    iii: existsSync(new URL('./.iii', import.meta.url)),
    built: lines.includes('./index.js') ? lines.some((line) => /^\.\/(.*\/)?\.iii(\/|$)/i.test(line)) : null,
  }
})
