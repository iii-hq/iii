import { createFileRoute } from '@tanstack/react-router'
import { FlowPage } from '@/components/flow/flow-page'

export const Route = createFileRoute('/flow')({
  component: FlowPage,
})
