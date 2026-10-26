import * as fs from 'node:fs'
import * as os from 'node:os'
import * as path from 'node:path'

/**
 * Writes `~/.iii/run/worker-<pid>-<n>.json` so `iii ps` can find this worker
 * however it was started, one file per client. Compose already marks its
 * children through their environment; this covers a worker run straight from
 * a terminal.
 *
 * Best effort: a home directory that cannot be written only makes the worker
 * harder to find. A process killed before it removes the marker leaves it
 * behind, and `iii ps` discards any marker whose process is gone.
 *
 * @returns A function that removes the marker.
 */
export function announceProcess(
  url: string,
  name: string,
  dir = path.join(os.homedir(), '.iii', 'run'),
): () => void {
  const file = path.join(dir, `worker-${process.pid}-${++announced}.json`)
  try {
    fs.mkdirSync(dir, { recursive: true })
    fs.writeFileSync(
      file,
      JSON.stringify({ pid: process.pid, url, name, sdk: 'node', started_at: Date.now() }),
    )
  } catch {
    return () => {}
  }
  // One exit listener for the process, however many clients it creates.
  if (markers.size === 0) {
    process.once('exit', () => {
      for (const marker of markers) remove(marker)
    })
  }
  markers.add(file)
  return () => remove(file)
}

const markers = new Set<string>()
let announced = 0

function remove(file: string): void {
  try {
    fs.rmSync(file, { force: true })
  } catch {
    // gone already, or never ours to remove
  }
}
