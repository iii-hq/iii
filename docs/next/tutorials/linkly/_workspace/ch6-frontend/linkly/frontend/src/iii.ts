import { registerWorker } from 'iii-browser-sdk'

// The browser is a worker. It connects to the RBAC-gated listener (`:3110`),
// not the trusted port that local workers use. The token in the query string
// is validated by `link::auth_browser` on every connection.
const TOKEN = import.meta.env.VITE_LINKLY_TOKEN ?? 'dev-token'

export const worker = registerWorker(`ws://localhost:3110?token=${encodeURIComponent(TOKEN)}`)
