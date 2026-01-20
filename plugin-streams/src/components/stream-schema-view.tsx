import { cn } from '@motiadev/ui'
import { AlertCircle, CheckCircle2, Hash, List, ToggleLeft, Type } from 'lucide-react'
import { memo, useState } from 'react'

interface SchemaProperty {
  type?: string
  enum?: string[]
  description?: string
}

interface SchemaObject {
  type?: string
  properties?: Record<string, SchemaProperty>
  required?: string[]
  additionalProperties?: boolean
  anyOf?: SchemaObject[]
}

interface StreamSchemaViewProps {
  schema: unknown
}

const getTypeIcon = (type?: string) => {
  switch (type) {
    case 'string':
      return <Type className="h-3.5 w-3.5 text-blue-500" />
    case 'number':
    case 'integer':
      return <Hash className="h-3.5 w-3.5 text-green-500" />
    case 'boolean':
      return <ToggleLeft className="h-3.5 w-3.5 text-orange-500" />
    case 'array':
      return <List className="h-3.5 w-3.5 text-purple-500" />
    default:
      return <AlertCircle className="h-3.5 w-3.5 text-muted-foreground" />
  }
}

const getTypeColor = (type?: string) => {
  switch (type) {
    case 'string':
      return 'text-blue-500 bg-blue-500/10'
    case 'number':
    case 'integer':
      return 'text-green-500 bg-green-500/10'
    case 'boolean':
      return 'text-orange-500 bg-orange-500/10'
    case 'array':
      return 'text-purple-500 bg-purple-500/10'
    default:
      return 'text-muted-foreground bg-muted'
  }
}

interface SchemaTypeCardProps {
  schema: SchemaObject
  title?: string
  index?: number
}

const SchemaTypeCard = memo<SchemaTypeCardProps>(({ schema, title, index }) => {
  const properties = schema.properties || {}
  const required = schema.required || []
  const propEntries = Object.entries(properties)

  const inferredName =
    title ||
    (properties.metric
      ? 'Analytics'
      : properties.ticketId && properties.customerPhone
        ? 'Escalation'
        : properties.orderName && properties.aiDecision
          ? 'RefundRequest'
          : properties.lastMessage
            ? 'Conversation'
            : index !== undefined
              ? `Type ${index + 1}`
              : 'Object')

  return (
    <div className="rounded-lg border border-border bg-card overflow-hidden">
      <div className="px-3 py-2 bg-muted/50 border-b border-border flex items-center justify-between">
        <span className="text-sm font-medium text-foreground">{inferredName}</span>
        <span className="text-xs text-muted-foreground">{propEntries.length} fields</span>
      </div>
      <div className="divide-y divide-border/50">
        {propEntries.map(([name, prop]) => {
          const isRequired = required.includes(name)
          const propData = prop as SchemaProperty

          return (
            <div key={name} className="flex items-center gap-3 px-3 py-2 hover:bg-muted/30 transition-colors">
              <div className="flex items-center gap-2 min-w-[140px]">
                {getTypeIcon(propData.type)}
                <span className={cn('text-sm font-mono', isRequired ? 'text-foreground' : 'text-muted-foreground')}>
                  {name}
                </span>
                {isRequired && (
                  <span title="Required">
                    <CheckCircle2 className="h-3 w-3 text-green-500" />
                  </span>
                )}
              </div>
              <div className="flex items-center gap-2 flex-1">
                <span className={cn('text-xs px-2 py-0.5 rounded-full font-medium', getTypeColor(propData.type))}>
                  {propData.type || 'unknown'}
                </span>
                {propData.enum && (
                  <span className="text-xs text-muted-foreground">
                    {propData.enum.slice(0, 3).join(' | ')}
                    {propData.enum.length > 3 && ` +${propData.enum.length - 3}`}
                  </span>
                )}
              </div>
            </div>
          )
        })}
      </div>
    </div>
  )
})
SchemaTypeCard.displayName = 'SchemaTypeCard'

export const StreamSchemaView = memo<StreamSchemaViewProps>(({ schema: rawSchema }) => {
  const [expandAll, setExpandAll] = useState(true)

  const schema = rawSchema as SchemaObject | null

  if (!schema) {
    return (
      <div className="p-4 bg-muted/30 rounded-lg text-center">
        <p className="text-sm text-muted-foreground">No schema defined</p>
      </div>
    )
  }

  if (schema.anyOf && Array.isArray(schema.anyOf)) {
    return (
      <div className="space-y-3">
        <div className="flex items-center justify-between mb-2">
          <p className="text-xs text-muted-foreground">
            This stream accepts {schema.anyOf.length} different data types:
          </p>
          <button
            type="button"
            onClick={() => setExpandAll(!expandAll)}
            className="text-xs text-primary hover:underline"
          >
            {expandAll ? 'Collapse all' : 'Expand all'}
          </button>
        </div>
        {expandAll && (
          <div className="grid grid-cols-2 gap-3">
            {schema.anyOf.map((typeSchema, index) => (
              <SchemaTypeCard key={`type-${index}`} schema={typeSchema as SchemaObject} index={index} />
            ))}
          </div>
        )}
      </div>
    )
  }

  if (schema.properties) {
    return (
      <div className="grid grid-cols-2 gap-3">
        <SchemaTypeCard schema={schema} />
      </div>
    )
  }

  return (
    <div className="p-4 bg-muted/30 rounded-lg text-center">
      <p className="text-sm text-muted-foreground">No schema properties defined</p>
    </div>
  )
})
StreamSchemaView.displayName = 'StreamSchemaView'
