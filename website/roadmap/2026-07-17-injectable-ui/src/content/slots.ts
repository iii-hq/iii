/* slots — the four registrars: mount point, exact props, host-owned walls. */

export type SlotId = 'composer' | 'calls' | 'pages' | 'forms'

export const SLOT_OPTIONS: { value: SlotId; label: string }[] = [
  { value: 'composer', label: 'composer.actions' },
  { value: 'calls', label: 'functionCalls' },
  { value: 'pages', label: 'pages' },
  { value: 'forms', label: 'configForms' },
]

export interface SlotSpec {
  title: string
  mount: string
  code: string
  walls: string[]
}

export const SLOTS: Record<SlotId, SlotSpec> = {
  composer: {
    title: 'extend the chat composer',
    mount:
      'a SlotOutlet appended to the composer footer’s left picker group, after the first-party pickers (Composer.tsx:332-375).',
    code: `interface ComposerActionProps {
  // append text to the draft as its own paragraph,
  // focus the editor, place the caret after it
  appendText(text: string): void
  // true while a submission is in flight
  busy: boolean
}`,
    walls: [
      'cannot mutate ComposerSubmitPayload — it has no extension field',
      'cannot touch lifted conversation state (mode, model, memory bank, working dir)',
      'acts through appendText or host.iii bus calls into its own worker',
    ],
  },
  calls: {
    title: 'override how a function call renders',
    mount:
      'FunctionCallCard dispatch order: injected renderers first (registration order), then the 13 first-party families, then the json fallback. first non-null wins; applies in chat and the traces span tab.',
    code: `interface FunctionCallRenderer {
  id: string                    // "state/page.js#renderer"
  isMatch(functionId: string): boolean
  tryRender(m: FunctionCallMessage): ReactNode | null
  tryRenderRunning?(m: FunctionCallMessage): ReactNode | null
  tryRenderPreview?(m: FunctionCallMessage): ReactNode | null
  FunctionIdLabel?(p: { functionId: string }): ReactNode
  primaryTabLabel?: string
}`,
    walls: [
      'errors parse before success — host semantics, unchanged',
      'the approval bar is host-rendered, never renderer-rendered',
      'null always means "fall through" — overriding a built-in is matching its ids',
    ],
  },
  pages: {
    title: 'contribute whole pages',
    mount:
      'route #/ext/<id> — a new hash-router prefix deliberately outside the first-party View union, so injected pages can never shadow #/traces or #/workers. nav options append at runtime.',
    code: `interface PageRegistration {
  id: string                // "<worker>-<name>", unique per tab
  title: string             // nav label
  render: React.ComponentType
}`,
    walls: [
      'dispose while the page is active → router falls back to #/traces',
      'duplicate id: last registration wins, console.warn names both paths',
      'windows = host.components.Dialog rendered inside any page or slot',
    ],
  },
  forms: {
    title: 'override a configuration form',
    mount:
      'the form region inside WorkerEditor — the SchemaForm render when a schema exists, or the "no editable configuration" EmptyState when none does. exact configuration-id match.',
    code: `interface ConfigFormProps {
  id: string                    // configuration id, e.g. "state"
  schema: JsonSchema | null     // null: value but no schema —
                                // the branch SchemaForm cannot render
  value: JsonValue
  onChange(next: JsonValue): void
  errors?: ReadonlyMap<string, string>
  focusField?: readonly string[]
}`,
    walls: [
      'dialog chrome, dirty-state, save/reset, error mapping stay host-owned',
      'an override cannot break persistence — it replaces what is drawn',
      'honoring focusField deep-links is the override’s job, by design',
    ],
  },
}
