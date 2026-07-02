/* coexistence — rbac-proxy vs the engine-native worker-gateway (README contrast). */

export interface CompareRow {
  property: string
  gateway: string
  proxy: string
  /** true when the proxy column is the meaningful win (gets the accent) */
  win?: boolean
}

export const COMPARE_ROWS: CompareRow[] = [
  { property: 'where rbac runs', gateway: 'in the engine process', proxy: 'a separate process / pod', win: true },
  {
    property: 'front a remote or managed engine',
    gateway: 'no, it is engine config',
    proxy: 'yes, a worker pointed at a url',
    win: true,
  },
  { property: 'deploy / scale / harden alone', gateway: 'no', proxy: 'yes', win: true },
  { property: 'blast radius of an auth bug', gateway: 'the engine', proxy: 'the proxy only', win: true },
  {
    property: 'discovery result filtering',
    gateway: 'partial: 2 of 8, in-process',
    proxy: 'all 8, client-side',
    win: true,
  },
  { property: 'wire protocol', gateway: 'unchanged', proxy: 'unchanged' },
  { property: 'migration between the two', gateway: 'same field set', proxy: 'a config copy', win: true },
]
