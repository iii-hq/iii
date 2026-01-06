'use client';

import { useEffect, useState } from 'react';
import { Card, CardContent, CardHeader, CardTitle, Badge, Table, TableHeader, TableBody, TableRow, TableHead, TableCell, Button, Input } from "@/components/ui/card";
import { Server, Search, RefreshCw, Activity, Clock, CheckCircle, XCircle, Code2, Eye, EyeOff } from "lucide-react";
import { fetchFunctions, fetchWorkers, FunctionInfo, WorkerInfo } from "@/lib/api";

export default function HandlersPage() {
  const [functions, setFunctions] = useState<FunctionInfo[]>([]);
  const [workers, setWorkers] = useState<WorkerInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [searchQuery, setSearchQuery] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [showSystem, setShowSystem] = useState(false);

  const loadData = async () => {
    setLoading(true);
    setError(null);
    try {
      const [functionsData, workersData] = await Promise.all([
        fetchFunctions(),
        fetchWorkers()
      ]);
      setFunctions(functionsData.functions);
      setWorkers(workersData.workers);
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to load data');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    loadData();
  }, []);

  const userFunctions = functions.filter(f => !f.internal);
  const systemFunctions = functions.filter(f => f.internal);

  const filteredFunctions = functions.filter(f => {
    if (!showSystem && f.internal) return false;
    if (searchQuery && !f.path.toLowerCase().includes(searchQuery.toLowerCase())) return false;
    return true;
  });

  const groupedFunctions = filteredFunctions.reduce((acc, fn) => {
    const parts = fn.path.split('.');
    const group = parts.length > 1 ? parts[0] : 'core';
    if (!acc[group]) acc[group] = [];
    acc[group].push(fn);
    return acc;
  }, {} as Record<string, FunctionInfo[]>);

  const groups = Object.keys(groupedFunctions).sort();

  return (
    <div className="p-6 space-y-6">
      {}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-semibold tracking-tight">Functions</h1>
          <p className="text-xs text-muted mt-1 tracking-wide">
            Your registered functions and worker processes
            {systemFunctions.length > 0 && !showSystem && (
              <span className="text-muted/60 ml-2">({systemFunctions.length} system hidden)</span>
            )}
          </p>
        </div>
        <div className="flex items-center gap-2">
          <Button 
            variant={showSystem ? "accent" : "ghost"} 
            size="sm" 
            onClick={() => setShowSystem(!showSystem)}
            title={showSystem ? "Hide system functions" : "Show system functions"}
          >
            {showSystem ? <EyeOff className="w-3 h-3 mr-2" /> : <Eye className="w-3 h-3 mr-2" />}
            {showSystem ? "Hide System" : "Show System"}
          </Button>
          <Button variant="outline" size="sm" onClick={loadData} disabled={loading}>
            <RefreshCw className={`w-3 h-3 mr-2 ${loading ? 'animate-spin' : ''}`} />
            Refresh
          </Button>
        </div>
      </div>

      {}
      {error && (
        <div className="bg-error/10 border border-error/30 rounded px-4 py-3 text-sm text-error">
          {error}
        </div>
      )}

      {}
      <div className="grid gap-4 grid-cols-2 lg:grid-cols-4">
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider">Your Functions</div>
                <div className="text-2xl font-semibold mt-1">{userFunctions.length}</div>
                {showSystem && systemFunctions.length > 0 && (
                  <div className="text-[9px] text-muted mt-0.5">+{systemFunctions.length} system</div>
                )}
              </div>
              <Code2 className="w-5 h-5 text-muted" />
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider">Workers</div>
                <div className="text-2xl font-semibold mt-1">{workers.length}</div>
              </div>
              <Server className="w-5 h-5 text-muted" />
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider">Services</div>
                <div className="text-2xl font-semibold mt-1">{groups.length}</div>
              </div>
              <Activity className="w-5 h-5 text-muted" />
            </div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="p-4">
            <div className="flex items-center justify-between">
              <div>
                <div className="text-[10px] text-muted uppercase tracking-wider">Status</div>
                <div className="text-lg font-semibold mt-1 text-success flex items-center gap-2">
                  <CheckCircle className="w-4 h-4" />
                  Active
                </div>
              </div>
            </div>
          </CardContent>
        </Card>
      </div>

      {}
      <Card>
        <CardHeader className="flex flex-row items-center justify-between">
          <CardTitle>Registered Functions</CardTitle>
          <div className="flex items-center gap-2">
            <div className="relative">
              <Search className="w-3 h-3 absolute left-3 top-1/2 -translate-y-1/2 text-muted" />
              <Input 
                placeholder="Search functions..."
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                className="pl-8 w-48"
              />
            </div>
          </div>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="flex items-center justify-center py-8 text-muted">
              <RefreshCw className="w-4 h-4 animate-spin mr-2" />
              Loading functions...
            </div>
          ) : filteredFunctions.length === 0 ? (
            <div className="flex items-center justify-center py-8 text-muted">
              No functions found
            </div>
          ) : (
            <div className="space-y-6">
              {groups.map(group => (
                <div key={group}>
                  <div className="flex items-center gap-2 mb-3">
                    <Badge variant="outline" className="text-[10px] uppercase">{group}</Badge>
                    <span className="text-[10px] text-muted">{groupedFunctions[group].length} functions</span>
                  </div>
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Function Path</TableHead>
                        <TableHead>Description</TableHead>
                        <TableHead>Status</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {groupedFunctions[group].map((fn) => (
                        <TableRow key={fn.path}>
                          <TableCell className="font-mono font-medium text-yellow">{fn.path}</TableCell>
                          <TableCell className="text-muted">
                            {fn.description || '—'}
                          </TableCell>
                          <TableCell>
                            <div className="flex items-center gap-2">
                              <div className="w-2 h-2 rounded-full bg-success" />
                              <span className="text-xs">Registered</span>
                            </div>
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {}
      {workers.length > 0 && (
        <Card>
          <CardHeader>
            <CardTitle>Connected Workers</CardTitle>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Worker ID</TableHead>
                  <TableHead>Functions</TableHead>
                  <TableHead>Status</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {workers.map((worker) => (
                  <TableRow key={worker.id}>
                    <TableCell className="font-mono">{worker.id}</TableCell>
                    <TableCell>
                      <div className="flex flex-wrap gap-1">
                        {worker.functions.slice(0, 5).map(fn => (
                          <Badge key={fn} variant="outline" className="text-[10px]">{fn}</Badge>
                        ))}
                        {worker.functions.length > 5 && (
                          <Badge variant="outline" className="text-[10px]">+{worker.functions.length - 5}</Badge>
                        )}
                      </div>
                    </TableCell>
                    <TableCell>
                      <Badge variant="success">CONNECTED</Badge>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}
    </div>
  );
}
