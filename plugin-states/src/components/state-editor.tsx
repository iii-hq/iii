import { Button } from '@motiadev/ui'
import { AlertCircle, Check, Loader2, Save } from 'lucide-react'
import type React from 'react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { StateItem } from '../types/state'
import { JsonEditor } from './json-editor'

type Props = {
  state: StateItem
}

export const StateEditor: React.FC<Props> = ({ state }) => {
  const [isRequestLoading, setIsRequestLoading] = useState(false)
  const [isValid, setIsValid] = useState(true)
  const [jsonValue, setJsonValue] = useState(JSON.stringify(state.value, null, 2))
  const [hasChanges, setHasChanges] = useState(false)
  const [saveStatus, setSaveStatus] = useState<'idle' | 'success' | 'error'>('idle')
  const lastSavedValue = useRef(JSON.stringify(state.value, null, 2))
  useEffect(() => {
    setJsonValue(JSON.stringify(state.value, null, 2))
  }, [state.value])

  const handleJsonChange = useCallback((value: string) => {
    setHasChanges(value !== lastSavedValue.current)
    setJsonValue(value)
    setSaveStatus('idle')
  }, [])

  const handleSave = async () => {
    if (!isValid || !hasChanges) return

    try {
      setIsRequestLoading(true)
      setSaveStatus('idle')

      const response = await fetch('/__motia/state', {
        method: 'POST',
        headers: {
          'Content-Type': 'application/json',
        },
        body: JSON.stringify({
          key: state.key,
          groupId: state.groupId,
          value: JSON.parse(jsonValue),
        }),
      })

      if (!response.ok) {
        throw new Error(`HTTP error! status: ${response.status}`)
      }

      lastSavedValue.current = jsonValue
      setSaveStatus('success')
      setHasChanges(false)

      setTimeout(() => {
        setSaveStatus('idle')
      }, 3000)
    } catch (error) {
      console.error('Failed to save state:', error)
      setSaveStatus('error')
    } finally {
      setIsRequestLoading(false)
    }
  }

  const resetChanges = useCallback(() => {
    setJsonValue(JSON.stringify(state.value, null, 2))
    setHasChanges(false)
    setSaveStatus('idle')
  }, [state.value])

  const statusView = useMemo(() => {
    if (saveStatus === 'success') {
      return (
        <div className="bg-green-50 dark:bg-green-950/20 border border-green-200 dark:border-green-800 rounded-lg p-2">
          <div className="flex items-center gap-2 text-green-700 dark:text-green-400 text-sm">
            <Check className="w-4 h-4" />
            State saved successfully!
          </div>
        </div>
      )
    }

    if (saveStatus === 'error') {
      return (
        <div className="bg-red-50 dark:bg-red-950/20 border border-red-200 dark:border-red-800 rounded-lg p-2">
          <div className="flex items-center gap-2 text-red-700 dark:text-red-400 text-sm">
            <AlertCircle className="w-4 h-4" />
            Failed to save state. Please try again.
          </div>
        </div>
      )
    }

    return (
      <div className="text-xs text-muted-foreground">
        {hasChanges ? (
          <span className="flex items-center gap-1">
            <div className="w-2 h-2 bg-orange-500 rounded-full"></div>
            Unsaved changes
          </span>
        ) : (
          <span className="flex items-center gap-1">
            <div className="w-2 h-2 bg-green-500 rounded-full"></div>
            Up to date
          </span>
        )}
      </div>
    )
  }, [saveStatus, hasChanges])

  return (
    <div className="flex flex-col gap-2 h-full">
      <p className="text-xs text-muted-foreground">Modify the state value using the JSON editor below.</p>
      <div className="space-y-3 pt-2 flex flex-col">
        <div className="relative flex-1">
          <JsonEditor
            value={jsonValue}
            onChange={handleJsonChange}
            onValidate={setIsValid}
            height={'calc(100vh - 300px)'}
          />

          {!isValid && (
            <div className="absolute top-2 right-2 bg-destructive/90 text-destructive-foreground px-2 py-1 rounded text-xs flex items-center gap-1">
              <AlertCircle className="w-3 h-3" />
              Invalid JSON
            </div>
          )}
        </div>
      </div>

      <div className="flex items-center justify-between pt-2">
        {statusView}

        <div className="flex items-center gap-2">
          {hasChanges && (
            <Button variant="secondary" onClick={resetChanges} disabled={isRequestLoading}>
              Reset
            </Button>
          )}

          <Button
            onClick={handleSave}
            variant="accent"
            disabled={isRequestLoading || !isValid || !hasChanges}
            data-testid="state-save-button"
          >
            {isRequestLoading ? (
              <>
                <Loader2 className="w-3 h-3 animate-spin mr-1" />
                Saving...
              </>
            ) : (
              <>
                <Save className="w-3 h-3 mr-1" />
                Save Changes
              </>
            )}
          </Button>
        </div>
      </div>
    </div>
  )
}
