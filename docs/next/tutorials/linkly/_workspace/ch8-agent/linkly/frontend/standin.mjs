// Node stand-in for the browser worker (NOT user-facing). The browser SDK runs
// on Node's global WebSocket, so this exercises the same mechanics the Vite app
// uses, without a real browser. Logs `EVENT ...` lines the e2e asserts on.
import { registerWorker, TriggerAction } from 'iii-browser-sdk'

const worker = registerWorker(process.env.III_WM_URL ?? 'ws://localhost:3110?token=dev-token')

let clicks = 0
worker.registerFunction('ui::on_click', async (data) => {
  clicks += 1
  console.log(`EVENT click ${clicks} ${JSON.stringify(data)}`)
  return null
})
worker.registerTrigger({
  type: 'stream',
  function_id: 'ui::on_click',
  config: { stream_name: 'clicks', group_id: 'all' },
})

worker.registerFunction('user::confirm_destructive_op', async (data) => {
  console.log(`EVENT confirm-requested ${JSON.stringify(data)}`)
  // Browser-as-queue-producer: the confirm itself enqueues the delete.
  await worker.trigger({
    function_id: 'link::delete',
    payload: { code: data.code },
    action: TriggerAction.Enqueue({ queue: 'deletes' }),
  })
  return { confirmed: true }
})

await new Promise((r) => setTimeout(r, 1500))
const link = await worker.trigger({
  function_id: 'link::create',
  payload: { url: 'https://from-browser.example', code: 'browser1' },
})
console.log(`EVENT created ${JSON.stringify(link)}`)

setInterval(() => {}, 1000)
