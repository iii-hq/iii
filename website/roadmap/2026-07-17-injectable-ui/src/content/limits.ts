/* limits — v1 boundaries, verbatim from the spec's non-goals. */
import type { StatusVariant } from '@lib/components/schematic/StatusPanel'

export const LIMITS: { variant: StatusVariant; headline: string; detail: string }[] = [
  {
    variant: 'alert',
    headline: 'no sandboxing',
    detail:
      'injected scripts run with full console-origin privileges — deliberately the same trust level as any worker on the bus, which can already invoke arbitrary functions. hardened deployments gate trigger-type registration via rbac; a config kill switch (injectable_ui: false) removes the whole surface.',
  },
  {
    variant: 'warn',
    headline: 'the scope wrapper is not isolation',
    detail:
      'data-iii-ui scoping is styling hygiene. a hand-built style asset can still ship unscoped selectors; the console lints and warns, never rejects.',
  },
  {
    variant: 'info',
    headline: 'behavior modification is render-level',
    detail:
      'overrides replace what is drawn, never what happens: submit pipeline, save/reset, approval actions and error mapping stay host-owned. submit interception and pipeline hooks are named v2 work.',
  },
  {
    variant: 'info',
    headline: 'no react state preservation across reloads',
    detail:
      'reload = dispose + remount of the script’s slot contributions — vite without react-refresh. honest and cheap; react-refresh integration is future work.',
  },
  {
    variant: 'info',
    headline: 'no script-to-script imports',
    detail:
      'v1 shared deps are exactly react, react-dom, react-dom/client, react/jsx-runtime, @iii/console. no per-script import-map extensions; everything else gets bundled in.',
  },
  {
    variant: 'info',
    headline: 'no asset persistence in the console',
    detail:
      'the registry rebuilds from engine replay + sdk reconnect replay. an engine restart wipes triggers engine-wide; workers re-register on reconnect.',
  },
]
