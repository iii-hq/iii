/**
 * Wire-level names of the stream protocol (`iii-browser-sdk/protocol`): the
 * channel ref a stream travels as, and helpers that find refs in raw JSON.
 * Application code uses `InputStream` and `OutputStream` from
 * `iii-browser-sdk`; these are for code that reads or builds refs itself.
 */
export { ChannelDirection, ChannelItem } from './channels'
export type { StreamChannelRef } from './iii-types'
export { extractChannelRefs, isChannelRef } from './utils'
