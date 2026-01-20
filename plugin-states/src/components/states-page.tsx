import {
  Button,
  Checkbox,
  cn,
  Input,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from '@motiadev/ui'
import { RefreshCw, Search, Trash, X } from 'lucide-react'
import { useMemo, useState } from 'react'
import { useGetStateItems } from '../hooks/states-hooks'
import { useStatesStore } from '../stores/use-states-store'
import type { StateItem } from '../types/state'
import { StateSidebar } from './state-sidebar'

export const StatesPage = () => {
  const selectedStateId = useStatesStore((state) => state.selectedStateId)
  const selectStateId = useStatesStore((state) => state.selectStateId)
  const { items, deleteItems, refetch } = useGetStateItems()
  const [search, setSearch] = useState('')
  const filteredItems = useMemo(() => {
    return items.filter((item) => {
      return (
        item.groupId.toLowerCase().includes(search.toLowerCase()) ||
        item.key.toLowerCase().includes(search.toLowerCase())
      )
    })
  }, [items, search])
  const selectedItem = useMemo(
    () => (selectedStateId ? filteredItems.find((item) => `${item.groupId}:${item.key}` === selectedStateId) : null),
    [filteredItems, selectedStateId],
  )

  const [checkedItems, setCheckedItems] = useState<Set<string>>(new Set())
  const handleRowClick = (item: StateItem) => selectStateId(`${item.groupId}:${item.key}`)
  const onClose = () => selectStateId(undefined)
  const deleteStates = () => {
    deleteItems(Array.from(checkedItems))
    setCheckedItems(new Set())
  }

  const handleCheckboxChange = (item: StateItem) => {
    const isChecked = checkedItems.has(`${item.groupId}:${item.key}`)

    setCheckedItems((prev) => {
      const newSet = new Set(prev)
      if (isChecked) {
        newSet.delete(`${item.groupId}:${item.key}`)
      } else {
        newSet.add(`${item.groupId}:${item.key}`)
      }
      return newSet
    })
  }

  const toggleSelectAll = (checked: boolean) => {
    setCheckedItems((prev) => {
      const newSet = new Set(prev)

      if (checked) {
        filteredItems.forEach((item) => {
          newSet.add(`${item.groupId}:${item.key}`)
        })
      } else {
        filteredItems.forEach((item) => {
          newSet.delete(`${item.groupId}:${item.key}`)
        })
      }
      return newSet
    })
  }

  return (
    <>
      {selectedItem && <StateSidebar state={selectedItem} onClose={onClose} />}
      <div className="grid grid-rows-[auto_1fr] h-full" data-testid="states-container">
        <div className="flex p-2 border-b gap-2" data-testid="logs-search-container">
          <div className="flex-1 relative">
            <Input
              variant="shade"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="px-9! font-medium"
              placeholder="Search by Group ID or Key"
            />
            <Search className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50" />
            <X
              className="cursor-pointer absolute right-3 top-1/2 -translate-y-1/2 w-4 h-4 text-muted-foreground/50 hover:text-muted-foreground"
              onClick={() => setSearch('')}
            />
          </div>
          <Button variant="default" className="h-[34px]" disabled={checkedItems.size === 0} onClick={deleteStates}>
            <Trash /> Delete
          </Button>
          <Button variant="default" className="h-[34px]" onClick={refetch}>
            <RefreshCw className="w-4 h-4 text-muted-foreground" />
          </Button>
        </div>

        <Table>
          <TableHeader className="sticky top-0 bg-background/20 backdrop-blur-sm">
            <TableRow>
              <TableHead>
                <Checkbox onClick={(evt) => evt.stopPropagation()} onCheckedChange={toggleSelectAll} />
              </TableHead>
              <TableHead className="rounded-0">Group ID</TableHead>
              <TableHead>Key</TableHead>
              <TableHead>Type</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {filteredItems.map((item) => (
              <TableRow
                data-testid={`item-${item}`}
                key={`${item.groupId}:${item.key}`}
                onClick={() => handleRowClick(item)}
                className={cn(
                  'font-mono font-semibold cursor-pointer border-0',
                  selectedItem === item
                    ? 'bg-muted-foreground/10 hover:bg-muted-foreground/20'
                    : 'hover:bg-muted-foreground/10',
                )}
              >
                <TableCell onClick={(evt) => evt.stopPropagation()}>
                  <Checkbox
                    checked={checkedItems.has(`${item.groupId}:${item.key}`)}
                    onClick={() => handleCheckboxChange(item)}
                  />
                </TableCell>
                <TableCell className="hover:bg-transparent">{item.groupId}</TableCell>
                <TableCell className="hover:bg-transparent">{item.key}</TableCell>
                <TableCell className="hover:bg-transparent">{item.type}</TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </>
  )
}
