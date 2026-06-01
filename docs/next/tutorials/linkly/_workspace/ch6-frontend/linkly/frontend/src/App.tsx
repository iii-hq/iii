import { useEffect, useState } from 'react'
import { worker } from './iii.js'

type Click = { code: string; clicked_at: string }
type StreamEvent = {
  event: { type: 'create' | 'update' | 'delete'; data: Click }
}

export function App() {
  const [url, setUrl] = useState('')
  const [code, setCode] = useState('')
  const [created, setCreated] = useState<{ code: string; url: string } | null>(null)
  const [clicks, setClicks] = useState(0)
  const [latest, setLatest] = useState<Click | null>(null)

  // Subscribe to live clicks from the engine. The handler runs in the browser.
  useEffect(() => {
    const fn = worker.registerFunction('ui::on_click', async (event: StreamEvent) => {
      setClicks((n) => n + 1)
      setLatest(event.event.data)
      return null
    })
    const trig = worker.registerTrigger({
      type: 'stream',
      function_id: 'ui::on_click',
      config: { stream_name: 'clicks', group_id: 'all' },
    })
    return () => {
      trig.unregister()
      fn.unregister()
    }
  }, [])

  // Server-initiated confirmation. The server calls this when something is
  // about to be deleted; the browser prompts and returns the answer.
  useEffect(() => {
    const fn = worker.registerFunction(
      'user::confirm_destructive_op',
      async (data: { action: string; code: string }) => {
        const confirmed = window.confirm(`Confirm: ${data.action}?`)
        return { confirmed }
      },
    )
    return () => fn.unregister()
  }, [])

  async function onSubmit(e: React.FormEvent) {
    e.preventDefault()
    const link = await worker.trigger<{ url: string; code?: string }, { code: string; url: string }>({
      function_id: 'link::create',
      payload: { url, code: code || undefined },
    })
    setCreated(link)
    setUrl('')
    setCode('')
  }

  return (
    <main style={{ fontFamily: 'system-ui', maxWidth: 640, margin: '2rem auto', padding: '0 1rem' }}>
      <h1>Linkly</h1>

      <form onSubmit={onSubmit}>
        <label>
          URL
          <input value={url} onChange={(e) => setUrl(e.target.value)} required style={{ width: '100%' }} />
        </label>
        <label>
          Code (optional)
          <input value={code} onChange={(e) => setCode(e.target.value)} style={{ width: '100%' }} />
        </label>
        <button type="submit">Shorten</button>
      </form>

      {created && (
        <p>
          Created <code>{created.code}</code> → <code>{created.url}</code>. Try{' '}
          <a href={`http://localhost:3111/s/${created.code}`}>localhost:3111/s/{created.code}</a>.
        </p>
      )}

      <section>
        <h2>Live clicks: {clicks}</h2>
        {latest && (
          <p>
            Last: <code>{latest.code}</code> at <code>{latest.clicked_at}</code>
          </p>
        )}
      </section>
    </main>
  )
}
