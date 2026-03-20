import { createFileRoute, redirect } from '@tanstack/react-router'

export const Route = createFileRoute('/dead-letter')({
  beforeLoad: () => {
    throw redirect({ to: '/queues', search: {} })
  },
})
