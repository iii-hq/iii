import { Button, Checkbox, cn, Input } from '@motiadev/ui'
import { AlertCircle, Check, Trash2 } from 'lucide-react'
import { type FC, memo, useCallback, useRef, useState } from 'react'
import type { ConfigurationLitItem } from '../hooks/use-endpoint-configuration'

interface ConfigurationListItemProps {
  value: ConfigurationLitItem
  id: string
  onUpdate: (key: string, field: 'name' | 'value' | 'active', value: string | boolean) => void
  onRemove?: (key: string) => void
  required?: boolean
}

export const ConfigurationListItem: FC<ConfigurationListItemProps> = memo(
  ({ value, id, onUpdate, onRemove, required = false }) => {
    const [activeConfiguration, setActiveConfiguration] = useState<string | null>(null)
    const [valueToDelete, setValueToDelete] = useState<string | null>(null)
    const deleteTimeout = useRef<NodeJS.Timeout | undefined>(undefined)
    const isActive = activeConfiguration === id

    const onDelete = useCallback(
      (key: string) => {
        clearTimeout(deleteTimeout.current)
        setValueToDelete(key)
        deleteTimeout.current = setTimeout(() => {
          setValueToDelete(null)
        }, 5000)
      },
      [setValueToDelete],
    )

    const onMouseOver = useCallback(() => {
      if (required) return
      setActiveConfiguration(id)
    }, [id, required])

    const onMouseLeave = useCallback(() => {
      if (required) return
      setActiveConfiguration(null)
    }, [required])

    return (
      <div
        className={cn(
          'grid grid-cols-[auto_1fr_auto] items-center py-2 px-4 bg-muted/30 rounded-lg gap-2',
          isActive && 'bg-card',
        )}
        onMouseOver={onMouseOver}
        onMouseLeave={onMouseLeave}
      >
        <Checkbox
          checked={value.active}
          onCheckedChange={(checked: boolean) => onUpdate(id, 'active', checked)}
          disabled={required}
        >
          <Check className="h-3 w-3" />
        </Checkbox>
        <div className="grid grid-cols-2 gap-2">
          <Input
            readOnly={required}
            disabled={!value.active}
            variant={required ? 'outlineReadonly' : 'outline'}
            id={`name-${id}`}
            defaultValue={value.name}
            onBlur={(e: React.ChangeEvent<HTMLInputElement>) => onUpdate(id, 'name', e.target.value)}
            placeholder="Name"
            className="h-8"
          />
          <Input
            disabled={!value.active}
            variant={'outline'}
            id={`value-${id}`}
            defaultValue={value.value}
            onBlur={(e: React.FocusEvent<HTMLInputElement>) => onUpdate(id, 'value', e.target.value)}
            placeholder="Value"
            className="h-8"
          />
        </div>
        <Button
          variant="ghost"
          size="icon"
          onClick={() => {
            if (valueToDelete === id) {
              onRemove?.(id)
            } else {
              onDelete(id)
            }
          }}
          className={cn('h-6 w-6 opacity-0 transition-opacity duration-200', isActive && 'opacity-100')}
          title={valueToDelete === id && isActive ? 'Click again to remove' : 'Remove'}
        >
          {valueToDelete === id ? <AlertCircle className="h-3 w-3 text-destructive" /> : <Trash2 className="h-3 w-3" />}
        </Button>
      </div>
    )
  },
)
