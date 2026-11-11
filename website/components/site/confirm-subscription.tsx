'use client'

import { useRouter } from 'next/navigation'
import { useEffect, useState } from 'react'

import { Button } from '@/components/ui/button'
import { confirmSubscription } from '@/lib/subscribe'

/**
 * The Confirm button of /subscribe-confirm. The emailed link carries the token after `#`, so it never reaches a server
 * log; it is read once and dropped from the address bar. Only the click confirms, so a mail scanner that opens the link
 * subscribes no one.
 */
export function ConfirmSubscription() {
  const router = useRouter()
  const [token, setToken] = useState<string | null>(null)
  const [state, setState] = useState<'idle' | 'sending' | 'failed'>('idle')

  useEffect(() => {
    const found = new URLSearchParams(window.location.hash.slice(1)).get('token')
    if (!found) {
      router.replace('/subscribe-expired')
      return
    }
    setToken(found)
    window.history.replaceState(null, '', window.location.pathname)
  }, [router])

  async function confirm() {
    if (!token) return
    setState('sending')
    const result = await confirmSubscription(token)
    if (result === 'confirmed') router.push('/subscribed')
    else if (result === 'invalid') router.push('/subscribe-expired')
    else setState('failed')
  }

  return (
    <div className="space-y-3">
      <Button size="lg" className="px-4" onClick={confirm} disabled={!token || state === 'sending'}>
        {state === 'sending' ? 'Confirming…' : 'Confirm subscription'}
      </Button>
      <output aria-live="polite" className="block text-[13px] text-muted-foreground">
        {state === 'failed' ? 'Unable to confirm right now. Check your connection and try again.' : ''}
      </output>
    </div>
  )
}
