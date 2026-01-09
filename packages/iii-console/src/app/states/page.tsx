'use client';

import { useState, useEffect, useCallback, useMemo } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Button, Input } from "@/components/ui/card";
import { 
  Database, Search, RefreshCw, Eye, EyeOff,
  ChevronDown, ChevronRight, X, Folder, Key, 
  Plus, Send, Trash2, Copy, Check, Edit2, Save,
  ArrowUpDown, ArrowDown, ArrowUp
} from "lucide-react";
import { fetchStreams, StreamInfo, fetchStateGroups, fetchStateItems, setStateItem, deleteStateItem, StateItem, StateGroup } from "@/lib/api";
import { Pagination } from "@/components/ui/pagination";
import { JsonViewer } from "@/components/ui/json-viewer";

const DEVTOOLS_API = 'http://localhost:3111/_console';

export default function StatesPage() {
  const [streams, setStreams] = useState<StreamInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState('');
  const [showSystem, setShowSystem] = useState(false);
  const [selectedStreamId, setSelectedStreamId] = useState<string | null>(null);
  const [selectedGroupId, setSelectedGroupId] = useState<string | null>(null);
  const [groups, setGroups] = useState<StateGroup[]>([]);
  const [loadingGroups, setLoadingGroups] = useState(false);
  const [items, setItems] = useState<StateItem[]>([]);
  const [loadingItems, setLoadingItems] = useState(false);
  const [expandedStreams, setExpandedStreams] = useState<Set<string>>(new Set(['User States']));
  
  const [selectedItem, setSelectedItem] = useState<StateItem | null>(null);
  const [showAddModal, setShowAddModal] = useState(false);
  const [newKey, setNewKey] = useState('');
  const [newValue, setNewValue] = useState('');
  const [saving, setSaving] = useState(false);
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [editingItem, setEditingItem] = useState<string | null>(null);
  const [editValue, setEditValue] = useState('');
  
  const [sortField, setSortField] = useState<'key' | 'type'>('key');
  const [sortDirection, setSortDirection] = useState<'asc' | 'desc'>('asc');
  const [itemsPage, setItemsPage] = useState(1);
  const [itemsPageSize, setItemsPageSize] = useState(50);

  const loadData = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await fetchStreams();
      setStreams(data.streams);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load states');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadData();
  }, []);

  const groupedStreams = useMemo(() => {
    const filtered = streams.filter(s => {
      if (!showSystem && s.internal) return false;
      if (searchQuery && !s.id.toLowerCase().includes(searchQuery.toLowerCase())) return false;
      return true;
    });

    const grouped: Record<string, StreamInfo[]> = {};
    filtered.forEach(stream => {
      const groupName = stream.internal ? 'System States' : 'User States';
      if (!grouped[groupName]) grouped[groupName] = [];
      grouped[groupName].push(stream);
    });

    return grouped;
  }, [streams, showSystem, searchQuery]);

  const selectedStream = streams.find(s => s.id === selectedStreamId);

  const discoverGroups = useCallback(async (streamName: string) => {
    setLoadingGroups(true);
    setGroups([]);
    setSelectedGroupId(null);
    setItems([]);
    setSelectedItem(null);
    
    try {
      const data = await fetchStateGroups(streamName);
      const sortedGroups = [...data.groups].sort((a, b) => b.count - a.count);
      setGroups(sortedGroups);
      setLoadingGroups(false);

      const firstWithItems = sortedGroups.find(g => g.count > 0);
      if (firstWithItems) {
        setSelectedGroupId(firstWithItems.id);
        loadGroupItems(streamName, firstWithItems.id);
      } else if (sortedGroups.length > 0) {
        setSelectedGroupId(sortedGroups[0].id);
      }
    } catch {
      setLoadingGroups(false);
    }
  }, []);

  const loadGroupItems = async (streamName: string, groupId: string) => {
    setLoadingItems(true);
    setItems([]);
    setSelectedItem(null);
    try {
      const data = await fetchStateItems(streamName, groupId);
      setItems(data.items);
    } catch {
      setItems([]);
    } finally {
      setLoadingItems(false);
    }
  };

  useEffect(() => {
    if (selectedStreamId) {
      discoverGroups(selectedStreamId);
    }
  }, [selectedStreamId, discoverGroups]);

  const handleSelectStream = (streamId: string) => {
    setSelectedStreamId(selectedStreamId === streamId ? null : streamId);
    setItems([]);
    setSelectedItem(null);
  };

  const handleSelectGroup = (groupId: string) => {
    setSelectedGroupId(groupId);
    setItems([]);
    setSelectedItem(null);
    if (selectedStreamId) {
      loadGroupItems(selectedStreamId, groupId);
    }
  };

  const toggleStreamExpand = (groupName: string) => {
    setExpandedStreams(prev => {
      const next = new Set(prev);
      if (next.has(groupName)) next.delete(groupName);
      else next.add(groupName);
      return next;
    });
  };

  const handleAddItem = async () => {
    if (!selectedStreamId || !selectedGroupId || !newKey) return;
    
    setSaving(true);
    try {
      let value: unknown = newValue;
      try {
        value = JSON.parse(newValue);
      } catch {
        // Keep as string if not valid JSON
      }

      await setStateItem(selectedStreamId, selectedGroupId, newKey, value);
      setNewKey('');
      setNewValue('');
      setShowAddModal(false);
      loadGroupItems(selectedStreamId, selectedGroupId);
    } catch {
      // Handle error
    } finally {
      setSaving(false);
    }
  };

  const handleDeleteItem = async (item: StateItem) => {
    if (!selectedStreamId || !selectedGroupId) return;
    
    try {
      await deleteStateItem(selectedStreamId, selectedGroupId, item.key);
      loadGroupItems(selectedStreamId, selectedGroupId);
      if (selectedItem?.key === item.key) {
        setSelectedItem(null);
      }
    } catch {
      // Handle error
    }
  };

  const handleEditItem = async (item: StateItem) => {
    if (!selectedStreamId || !selectedGroupId) return;
    
    setSaving(true);
    try {
      let value: unknown = editValue;
      try {
        value = JSON.parse(editValue);
      } catch {
        // Keep as string if not valid JSON
      }

      await setStateItem(selectedStreamId, selectedGroupId, item.key, value);
      setEditingItem(null);
      setEditValue('');
      loadGroupItems(selectedStreamId, selectedGroupId);
    } catch {
      // Handle error
    } finally {
      setSaving(false);
    }
  };

  const copyToClipboard = async (text: string, id: string) => {
    await navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 2000);
  };

  const sortedItems = useMemo(() => {
    return [...items].sort((a, b) => {
      const aVal = sortField === 'key' ? a.key : a.type;
      const bVal = sortField === 'key' ? b.key : b.type;
      const comparison = aVal.localeCompare(bVal);
      return sortDirection === 'asc' ? comparison : -comparison;
    });
  }, [items, sortField, sortDirection]);

  // Pagination for items
  const totalItemPages = Math.max(1, Math.ceil(sortedItems.length / itemsPageSize));
  const paginatedItems = useMemo(() => {
    const start = (itemsPage - 1) * itemsPageSize;
    return sortedItems.slice(start, start + itemsPageSize);
  }, [sortedItems, itemsPage, itemsPageSize]);

  // Reset to page 1 when group changes or page size changes
  useEffect(() => {
    setItemsPage(1);
  }, [selectedGroupId, itemsPageSize]);

  const toggleSort = (field: 'key' | 'type') => {
    if (sortField === field) {
      setSortDirection(prev => prev === 'asc' ? 'desc' : 'asc');
    } else {
      setSortField(field);
      setSortDirection('asc');
    }
  };

  const userStreams = streams.filter(s => !s.internal);
  const systemStreams = streams.filter(s => s.internal);

  const totalItems = groups.reduce((sum, g) => sum + g.count, 0);

  // State for mobile sidebar visibility
  const [showMobileSidebar, setShowMobileSidebar] = useState(false);

  return (
    <div className="flex flex-col h-full bg-background text-foreground">
      {/* Header */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2 px-3 md:px-5 py-2 md:py-3 bg-dark-gray/30 border-b border-border">
        <div className="flex items-center gap-2 md:gap-4 flex-wrap">
          <h1 className="text-sm md:text-base font-semibold flex items-center gap-2">
            <Database className="w-4 h-4 text-blue-400" />
            States
          </h1>
          <div className="text-[10px] md:text-xs text-muted bg-dark-gray/50 px-2 py-0.5 md:py-1 rounded hidden sm:block">
            Key-Value Store
          </div>
          <div className="flex items-center gap-1.5 md:gap-2 px-1.5 md:px-2 py-0.5 md:py-1 rounded bg-dark-gray/50 text-[10px] md:text-xs text-muted">
            <Database className="w-3 h-3" />
            <span>{userStreams.length} stores</span>
          </div>
        </div>
        
        <div className="flex items-center gap-1.5 md:gap-2">
          {/* Mobile sidebar toggle */}
          <Button 
            variant="ghost" 
            size="sm" 
            onClick={() => setShowMobileSidebar(!showMobileSidebar)}
            className="h-7 text-xs md:hidden"
          >
            <Folder className="w-3 h-3 mr-1" />
            Stores
          </Button>
          <Button 
            variant={showSystem ? "accent" : "ghost"} 
            size="sm" 
            onClick={() => setShowSystem(!showSystem)}
            className="h-6 md:h-7 text-[10px] md:text-xs px-2"
          >
            {showSystem ? <Eye className="w-3 h-3 md:mr-1.5" /> : <EyeOff className="w-3 h-3 md:mr-1.5" />}
            <span className={`hidden md:inline ${showSystem ? '' : 'line-through opacity-60'}`}>System</span>
          </Button>
          <Button 
            variant="ghost" 
            size="sm" 
            onClick={loadData} 
            disabled={loading}
            className="h-6 md:h-7 text-[10px] md:text-xs text-muted hover:text-foreground px-2"
          >
            <RefreshCw className={`w-3 h-3 md:w-3.5 md:h-3.5 md:mr-1.5 ${loading ? 'animate-spin' : ''}`} />
            <span className="hidden md:inline">Refresh</span>
          </Button>
        </div>
      </div>

      {error && (
        <div className="mx-3 md:mx-4 mt-3 md:mt-4 bg-error/10 border border-error/30 rounded px-3 md:px-4 py-2 md:py-3 text-xs md:text-sm text-error">
          {error}
        </div>
      )}

      {/* Mobile sidebar overlay */}
      {showMobileSidebar && (
        <div className="md:hidden fixed inset-0 z-40 bg-black/60" onClick={() => setShowMobileSidebar(false)} />
      )}

      <div className={`flex-1 grid overflow-hidden 
        ${selectedItem 
          ? 'grid-cols-1 md:grid-cols-[240px_1fr] lg:grid-cols-[280px_1fr_320px]' 
          : 'grid-cols-1 md:grid-cols-[240px_1fr] lg:grid-cols-[280px_1fr]'
        }`}>
        {/* Left Sidebar - State Stores (hidden on mobile, shown via overlay) */}
        <div className={`
          flex flex-col h-full overflow-hidden border-r border-border bg-dark-gray/20
          fixed md:relative inset-y-0 left-0 z-50 w-[280px] md:w-auto
          transform transition-transform duration-300 ease-in-out
          ${showMobileSidebar ? 'translate-x-0' : '-translate-x-full md:translate-x-0'}
        `}>
          <div className="p-2 md:p-3 border-b border-border">
            <div className="flex items-center justify-between md:hidden mb-2">
              <span className="text-xs font-semibold">State Stores</span>
              <button onClick={() => setShowMobileSidebar(false)} className="p-1 hover:bg-dark-gray rounded">
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="relative">
              <Search className="absolute left-2.5 md:left-3 top-1/2 -translate-y-1/2 w-3.5 h-3.5 md:w-4 md:h-4 text-muted" />
              <Input
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search..."
                className="pl-8 md:pl-9 pr-8 md:pr-9 h-8 md:h-9 text-xs md:text-sm"
              />
              {searchQuery && (
                <button
                  onClick={() => setSearchQuery('')}
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-muted hover:text-foreground"
                >
                  <X className="w-4 h-4" />
                </button>
              )}
            </div>
          </div>

          <div className="flex-1 overflow-y-auto py-2">
            {loading ? (
              <div className="flex items-center justify-center h-32">
                <RefreshCw className="w-5 h-5 text-muted animate-spin" />
              </div>
            ) : Object.keys(groupedStreams).length === 0 ? (
              <div className="flex flex-col items-center justify-center h-64 px-4">
                <div className="w-12 h-12 mb-3 rounded-xl bg-dark-gray border border-border flex items-center justify-center">
                  <Database className="w-6 h-6 text-muted" />
                </div>
                <div className="text-sm font-medium mb-1">No state stores found</div>
                <div className="text-xs text-muted text-center">
                  Create state stores using the SDK
                </div>
              </div>
            ) : (
              Object.entries(groupedStreams)
                .sort(([a], [b]) => (a === 'System States' ? 1 : b === 'System States' ? -1 : a.localeCompare(b)))
                .map(([groupName, groupStreams]) => (
                  <div key={groupName} className="mb-1">
                    <button
                      onClick={() => toggleStreamExpand(groupName)}
                      className="w-full flex items-center gap-2 px-3 py-2 text-xs font-semibold text-muted uppercase tracking-wider hover:bg-dark-gray/30 transition-colors"
                    >
                      {expandedStreams.has(groupName) ? (
                        <ChevronDown className="w-3.5 h-3.5" />
                      ) : (
                        <ChevronRight className="w-3.5 h-3.5" />
                      )}
                      <span>{groupName}</span>
                      <span className="ml-auto text-[10px] bg-dark-gray px-1.5 py-0.5 rounded-full">
                        {groupStreams.length}
                      </span>
                    </button>

                    {expandedStreams.has(groupName) && (
                      <div className="space-y-0.5 pb-2">
                        {groupStreams.map(stream => (
                          <button
                            key={stream.id}
                            onClick={() => handleSelectStream(stream.id)}
                            className={`w-full flex items-center gap-2 px-3 py-2 mx-2 text-left transition-colors rounded-md
                              ${selectedStreamId === stream.id 
                                ? 'bg-blue-500/10 text-blue-400 border-l-2 border-blue-400' 
                                : 'text-foreground/80 hover:bg-dark-gray/50'
                              }
                            `}
                            style={{ width: 'calc(100% - 16px)' }}
                          >
                            <Database className={`h-4 w-4 shrink-0 ${selectedStreamId === stream.id ? 'text-blue-400' : 'text-muted'}`} />
                            <div className="flex-1 min-w-0">
                              <div className={`text-sm font-medium truncate ${selectedStreamId === stream.id ? 'text-blue-400' : ''}`}>
                                {stream.id}
                              </div>
                            </div>
                            {stream.groups.length > 0 && (
                              <span className="text-[10px] text-muted bg-dark-gray px-1.5 py-0.5 rounded">
                                {stream.groups.length}
                              </span>
                            )}
                          </button>
                        ))}
                      </div>
                    )}
                  </div>
                ))
            )}
          </div>
        </div>

        {/* Main Content - Items Table or Empty State */}
        {!selectedStream ? (
          <div className="flex flex-col items-center justify-center h-full bg-background">
            <div className="text-center max-w-md px-8">
              <div className="inline-flex items-center justify-center w-16 h-16 rounded-full bg-blue-500/10 mb-6">
                <Database className="w-8 h-8 text-blue-400" />
              </div>
              <h2 className="text-xl font-semibold mb-2">Select a State Store</h2>
              <p className="text-muted text-sm mb-6">
                Choose a state store from the sidebar to view and manage its key-value data.
              </p>
              <div className="text-left bg-dark-gray/30 rounded-lg p-4 text-xs">
                <div className="font-medium mb-2 text-foreground">State Stores contain:</div>
                <ul className="space-y-1.5 text-muted">
                  <li className="flex items-center gap-2">
                    <Folder className="w-3.5 h-3.5 text-blue-400" />
                    <span><strong className="text-foreground">Groups</strong> - Logical partitions for your data</span>
                  </li>
                  <li className="flex items-center gap-2">
                    <Key className="w-3.5 h-3.5 text-yellow" />
                    <span><strong className="text-foreground">Items</strong> - Key-value pairs stored in groups</span>
                  </li>
                </ul>
              </div>
              {streams.length > 0 && (
                <div className="mt-6 text-xs text-muted">
                  {streams.filter(s => !s.internal).length} user stores available
                </div>
              )}
            </div>
          </div>
        ) : (
          <div className="flex flex-col h-full overflow-hidden bg-background">
            {/* Stream Header */}
            <div className="flex items-center justify-between px-4 py-3 border-b border-border bg-dark-gray/30">
              <div className="flex items-center gap-3 min-w-0">
                <div className="p-2 rounded-lg bg-blue-500/10">
                  <Database className="h-5 w-5 text-blue-400" />
                </div>
                <div className="min-w-0">
                  <h2 className="text-base font-semibold text-foreground truncate">{selectedStream.id}</h2>
                  <div className="flex items-center gap-1.5 text-xs text-muted">
                    <Folder className="h-3 w-3" />
                    <span>{groups.length} groups</span>
                    <span className="text-border">•</span>
                    <span>{totalItems} items</span>
                  </div>
                </div>
              </div>

              <div className="flex items-center gap-2">
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => selectedStreamId && discoverGroups(selectedStreamId)}
                  disabled={loadingGroups}
                  className="h-7 w-7 p-0"
                >
                  <RefreshCw className={`w-4 h-4 ${loadingGroups ? 'animate-spin' : ''}`} />
                </Button>
                <Button
                  variant="ghost"
                  size="sm"
                  onClick={() => setSelectedStreamId(null)}
                  className="h-7 w-7 p-0"
                >
                  <X className="w-4 h-4" />
                </Button>
              </div>
            </div>

            {/* Groups */}
            <div className="px-4 py-3 border-b border-border bg-dark-gray/20">
              <div className="flex items-center justify-between mb-3">
                <div className="flex items-center gap-2">
                  <Folder className="h-4 w-4 text-muted" />
                  <h3 className="text-sm font-medium">Groups</h3>
                  {loadingGroups && <RefreshCw className="w-3.5 h-3.5 text-muted animate-spin" />}
                </div>
              </div>

              <div className="flex flex-wrap gap-2">
                {groups.map(group => (
                  <button
                    key={group.id}
                    onClick={() => handleSelectGroup(group.id)}
                    className={`flex items-center gap-2 px-3 py-2 rounded-md text-sm transition-all
                      ${selectedGroupId === group.id
                        ? 'bg-blue-500 text-white shadow-md'
                        : group.count > 0
                          ? 'bg-dark-gray text-foreground hover:bg-dark-gray/80'
                          : 'bg-dark-gray/50 text-muted hover:bg-dark-gray/80'
                      }
                    `}
                  >
                    <span className="font-medium capitalize">{group.id}</span>
                    <span className={`text-xs px-1.5 py-0.5 rounded-full min-w-[20px] text-center
                      ${selectedGroupId === group.id
                        ? 'bg-white/20'
                        : group.count > 0
                          ? 'bg-blue-500/20 text-blue-400'
                          : 'bg-background'
                      }
                    `}>
                      {group.count}
                    </span>
                  </button>
                ))}

                {groups.length === 0 && !loadingGroups && (
                  <p className="text-sm text-muted">No groups found.</p>
                )}
              </div>
            </div>

            {/* Items Header */}
            <div className="flex items-center justify-between px-4 py-2 border-b border-border bg-dark-gray/10">
              <div className="flex items-center gap-2 text-xs text-muted">
                <Key className="w-3.5 h-3.5" />
                <span>{items.length} items in {selectedGroupId || 'group'}</span>
              </div>

              <Button
                variant="outline"
                size="sm"
                onClick={() => setShowAddModal(true)}
                className="h-7 text-xs gap-1.5"
                disabled={!selectedGroupId}
              >
                <Plus className="w-3 h-3" />
                Add Item
              </Button>
            </div>

            {/* Items Table */}
            <div className="flex flex-col flex-1 overflow-hidden">
              <div className="flex-1 overflow-auto">
                {loadingItems ? (
                  <div className="flex items-center justify-center h-32">
                    <RefreshCw className="w-5 h-5 text-muted animate-spin" />
                  </div>
                ) : selectedGroupId ? (
                  items.length > 0 ? (
                  <>
                    <table className="w-full">
                      <thead className="sticky top-0 bg-dark-gray/80 backdrop-blur-sm">
                        <tr className="border-b border-border">
                          <th 
                            className="text-left text-xs font-medium text-muted uppercase tracking-wider px-4 py-2 cursor-pointer hover:text-foreground"
                            onClick={() => toggleSort('key')}
                          >
                            <div className="flex items-center gap-1">
                              Key
                              {sortField === 'key' ? (
                                sortDirection === 'asc' ? <ArrowUp className="w-3 h-3" /> : <ArrowDown className="w-3 h-3" />
                              ) : (
                                <ArrowUpDown className="w-3 h-3 opacity-30" />
                              )}
                            </div>
                          </th>
                          <th 
                            className="text-left text-xs font-medium text-muted uppercase tracking-wider px-4 py-2 cursor-pointer hover:text-foreground"
                            onClick={() => toggleSort('type')}
                          >
                            <div className="flex items-center gap-1">
                              Type
                              {sortField === 'type' ? (
                                sortDirection === 'asc' ? <ArrowUp className="w-3 h-3" /> : <ArrowDown className="w-3 h-3" />
                              ) : (
                                <ArrowUpDown className="w-3 h-3 opacity-30" />
                              )}
                            </div>
                          </th>
                          <th className="text-left text-xs font-medium text-muted uppercase tracking-wider px-4 py-2">
                            Preview
                          </th>
                          <th className="w-20"></th>
                        </tr>
                      </thead>
                      <tbody className="divide-y divide-border/30">
                        {paginatedItems.map((item) => (
                          <tr 
                            key={item.key}
                            onClick={() => setSelectedItem(item)}
                            className={`cursor-pointer transition-colors ${
                              selectedItem?.key === item.key 
                                ? 'bg-blue-500/10' 
                                : 'hover:bg-dark-gray/30'
                            }`}
                          >
                            <td className="px-4 py-3">
                              <div className="flex items-center gap-2">
                                <Key className="w-3.5 h-3.5 text-blue-400 shrink-0" />
                                <span className="font-mono text-sm font-medium truncate max-w-[200px]">
                                  {item.key}
                                </span>
                              </div>
                            </td>
                            <td className="px-4 py-3">
                              <Badge variant="outline" className="text-[10px]">
                                {item.type}
                              </Badge>
                            </td>
                            <td className="px-4 py-3">
                              <span className="text-xs text-muted font-mono truncate block max-w-[300px]">
                                {JSON.stringify(item.value).slice(0, 50)}
                                {JSON.stringify(item.value).length > 50 && '...'}
                              </span>
                            </td>
                            <td className="px-4 py-3">
                              <div className="flex items-center gap-1">
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    copyToClipboard(JSON.stringify(item.value, null, 2), item.key);
                                  }}
                                  className="p-1.5 rounded hover:bg-dark-gray"
                                  title="Copy JSON"
                                >
                                  {copiedId === item.key ? (
                                    <Check className="w-3.5 h-3.5 text-success" />
                                  ) : (
                                    <Copy className="w-3.5 h-3.5 text-muted" />
                                  )}
                                </button>
                                <button
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    handleDeleteItem(item);
                                  }}
                                  className="p-1.5 rounded hover:bg-dark-gray"
                                  title="Delete"
                                >
                                  <Trash2 className="w-3.5 h-3.5 text-error" />
                                </button>
                              </div>
                            </td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </>
                  ) : (
                  <div className="flex flex-col items-center justify-center h-64">
                    <div className="w-12 h-12 mb-3 rounded-xl bg-dark-gray border border-border flex items-center justify-center">
                      <Folder className="h-6 w-6 text-muted" />
                    </div>
                    <p className="text-sm font-medium mb-1">No items in this group</p>
                    <p className="text-xs text-muted mb-4">Add items to store key-value data</p>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={() => setShowAddModal(true)}
                      className="gap-1.5"
                    >
                      <Plus className="w-3 h-3" />
                      Add Item
                    </Button>
                  </div>
                )
              ) : (
                <div className="flex flex-col items-center justify-center h-64">
                  <Folder className="h-12 w-12 text-muted/50 mb-4" />
                  <p className="text-sm text-muted">Select a group to view items</p>
                </div>
              )}
              </div>
              
              {/* Pagination - Fixed at bottom */}
              {selectedGroupId && items.length > 0 && sortedItems.length > 0 && (
                <div className="flex-shrink-0 bg-background/95 backdrop-blur border-t border-border px-3 py-2">
                  <Pagination
                    currentPage={itemsPage}
                    totalPages={totalItemPages}
                    totalItems={sortedItems.length}
                    pageSize={itemsPageSize}
                    onPageChange={setItemsPage}
                    onPageSizeChange={setItemsPageSize}
                    pageSizeOptions={[25, 50, 100]}
                  />
                </div>
              )}
            </div>
          </div>
        )}

        {/* Right Sidebar - Item Details */}
        {selectedItem && (
          <div className="flex flex-col h-full overflow-hidden border-l border-border bg-dark-gray/10">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <div className="flex items-center gap-2 min-w-0">
                <Key className="w-4 h-4 text-blue-400 shrink-0" />
                <span className="font-mono text-sm font-medium truncate">{selectedItem.key}</span>
              </div>
              <button
                onClick={() => setSelectedItem(null)}
                className="p-1 rounded hover:bg-dark-gray"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="flex-1 overflow-auto p-4">
              <div className="space-y-4">
                <div>
                  <div className="text-xs text-muted uppercase tracking-wider mb-2">Key</div>
                  <div className="font-mono text-sm bg-dark-gray p-2 rounded">{selectedItem.key}</div>
                </div>

                <div>
                  <div className="text-xs text-muted uppercase tracking-wider mb-2">Type</div>
                  <Badge variant="outline">{selectedItem.type}</Badge>
                </div>

                <div>
                  <div className="flex items-center justify-between mb-2">
                    <span className="text-xs text-muted uppercase tracking-wider">Value</span>
                    <div className="flex items-center gap-1">
                      {editingItem === selectedItem.key ? (
                        <>
                          <button
                            onClick={() => handleEditItem(selectedItem)}
                            disabled={saving}
                            className="p-1 rounded hover:bg-dark-gray text-success"
                          >
                            <Save className="w-3.5 h-3.5" />
                          </button>
                          <button
                            onClick={() => {
                              setEditingItem(null);
                              setEditValue('');
                            }}
                            className="p-1 rounded hover:bg-dark-gray"
                          >
                            <X className="w-3.5 h-3.5" />
                          </button>
                        </>
                      ) : (
                        <>
                          <button
                            onClick={() => {
                              setEditingItem(selectedItem.key);
                              setEditValue(JSON.stringify(selectedItem.value, null, 2));
                            }}
                            className="p-1 rounded hover:bg-dark-gray"
                          >
                            <Edit2 className="w-3.5 h-3.5 text-muted" />
                          </button>
                          <button
                            onClick={() => copyToClipboard(JSON.stringify(selectedItem.value, null, 2), `detail-${selectedItem.key}`)}
                            className="p-1 rounded hover:bg-dark-gray"
                          >
                            {copiedId === `detail-${selectedItem.key}` ? (
                              <Check className="w-3.5 h-3.5 text-success" />
                            ) : (
                              <Copy className="w-3.5 h-3.5 text-muted" />
                            )}
                          </button>
                        </>
                      )}
                    </div>
                  </div>
                  {editingItem === selectedItem.key ? (
                    <textarea
                      value={editValue}
                      onChange={(e) => setEditValue(e.target.value)}
                      className="w-full h-64 px-3 py-2 bg-dark-gray border border-border rounded-md font-mono text-xs resize-none focus:outline-none focus:ring-1 focus:ring-blue-500"
                    />
                  ) : (
                    <div className="p-3 rounded-lg bg-dark-gray overflow-x-auto max-h-[400px] overflow-y-auto">
                      <JsonViewer data={selectedItem.value} collapsed={false} maxDepth={6} />
                    </div>
                  )}
                </div>

                <div className="pt-4 border-t border-border">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => handleDeleteItem(selectedItem)}
                    className="w-full gap-1.5 border-error/50 text-error hover:bg-error/10"
                  >
                    <Trash2 className="w-3 h-3" />
                    Delete Item
                  </Button>
                </div>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* Add Item Modal */}
      {showAddModal && (
        <div className="fixed inset-0 bg-background/80 backdrop-blur-sm flex items-center justify-center z-50">
          <div className="bg-background border border-border rounded-lg shadow-xl w-full max-w-md">
            <div className="flex items-center justify-between px-4 py-3 border-b border-border">
              <h3 className="font-semibold">Add State Item</h3>
              <button
                onClick={() => setShowAddModal(false)}
                className="p-1 rounded hover:bg-dark-gray"
              >
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="p-4 space-y-4">
              <div>
                <label className="text-xs text-muted block mb-1.5">Store / Group</label>
                <div className="flex items-center gap-2 text-sm font-mono bg-dark-gray/50 px-3 py-2 rounded">
                  <Database className="w-4 h-4 text-blue-400" />
                  <span>{selectedStreamId}</span>
                  <ChevronRight className="w-3 h-3 text-muted" />
                  <span className="text-blue-400">{selectedGroupId}</span>
                </div>
              </div>

              <div>
                <label className="text-xs text-muted block mb-1.5">Key</label>
                <Input
                  value={newKey}
                  onChange={(e) => setNewKey(e.target.value)}
                  placeholder="item-key"
                  className="font-mono"
                />
              </div>

              <div>
                <label className="text-xs text-muted block mb-1.5">Value (JSON or string)</label>
                <textarea
                  value={newValue}
                  onChange={(e) => setNewValue(e.target.value)}
                  placeholder='{"key": "value"}'
                  className="w-full h-32 px-3 py-2 bg-dark-gray border border-border rounded-md font-mono text-sm resize-none focus:outline-none focus:ring-1 focus:ring-blue-500"
                />
              </div>
            </div>

            <div className="flex items-center justify-end gap-2 px-4 py-3 border-t border-border">
              <Button
                variant="ghost"
                size="sm"
                onClick={() => setShowAddModal(false)}
              >
                Cancel
              </Button>
              <Button
                variant="accent"
                size="sm"
                onClick={handleAddItem}
                disabled={!newKey || saving}
                className="gap-1.5"
              >
                {saving ? (
                  <RefreshCw className="w-3 h-3 animate-spin" />
                ) : (
                  <Send className="w-3 h-3" />
                )}
                Save
              </Button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
