'use client';

import { useState, useMemo, useCallback } from 'react';
import { ChevronRight, ChevronDown, Copy, Check } from 'lucide-react';

interface JsonViewerProps {
  data: unknown;
  collapsed?: boolean;
  depth?: number;
  maxDepth?: number;
  className?: string;
}

// Color scheme for syntax highlighting
const COLORS = {
  key: 'text-purple-400',
  string: 'text-green-400',
  number: 'text-cyan-400',
  boolean: 'text-yellow',
  null: 'text-muted',
  bracket: 'text-foreground/60',
  punctuation: 'text-foreground/40',
};

function JsonValue({ value, depth, maxDepth, defaultCollapsed }: { 
  value: unknown; 
  depth: number; 
  maxDepth: number;
  defaultCollapsed: boolean;
}) {
  const [isCollapsed, setIsCollapsed] = useState(defaultCollapsed && depth > 0);

  if (value === null) {
    return <span className={COLORS.null}>null</span>;
  }

  if (value === undefined) {
    return <span className={COLORS.null}>undefined</span>;
  }

  if (typeof value === 'string') {
    // Truncate long strings
    const displayValue = value.length > 200 ? `${value.slice(0, 200)}...` : value;
    return <span className={COLORS.string}>"{displayValue}"</span>;
  }

  if (typeof value === 'number') {
    return <span className={COLORS.number}>{value}</span>;
  }

  if (typeof value === 'boolean') {
    return <span className={COLORS.boolean}>{value ? 'true' : 'false'}</span>;
  }

  if (Array.isArray(value)) {
    if (value.length === 0) {
      return <span className={COLORS.bracket}>[]</span>;
    }

    if (depth >= maxDepth) {
      return (
        <span className={COLORS.bracket}>
          [{value.length} items]
        </span>
      );
    }

    return (
      <span>
        <button 
          onClick={() => setIsCollapsed(!isCollapsed)}
          className="inline-flex items-center hover:bg-dark-gray/50 rounded px-0.5 -ml-0.5"
        >
          {isCollapsed ? (
            <ChevronRight className="w-3 h-3 text-muted" />
          ) : (
            <ChevronDown className="w-3 h-3 text-muted" />
          )}
        </button>
        <span className={COLORS.bracket}>[</span>
        {isCollapsed ? (
          <span className="text-muted italic">{value.length} items</span>
        ) : (
          <>
            {value.map((item, i) => (
              <div key={i} style={{ paddingLeft: '1rem' }}>
                <JsonValue value={item} depth={depth + 1} maxDepth={maxDepth} defaultCollapsed={defaultCollapsed} />
                {i < value.length - 1 && <span className={COLORS.punctuation}>,</span>}
              </div>
            ))}
          </>
        )}
        <span className={COLORS.bracket}>]</span>
      </span>
    );
  }

  if (typeof value === 'object') {
    const entries = Object.entries(value);
    
    if (entries.length === 0) {
      return <span className={COLORS.bracket}>{'{}'}</span>;
    }

    if (depth >= maxDepth) {
      return (
        <span className={COLORS.bracket}>
          {'{'}...{'}'}
        </span>
      );
    }

    return (
      <span>
        <button 
          onClick={() => setIsCollapsed(!isCollapsed)}
          className="inline-flex items-center hover:bg-dark-gray/50 rounded px-0.5 -ml-0.5"
        >
          {isCollapsed ? (
            <ChevronRight className="w-3 h-3 text-muted" />
          ) : (
            <ChevronDown className="w-3 h-3 text-muted" />
          )}
        </button>
        <span className={COLORS.bracket}>{'{'}</span>
        {isCollapsed ? (
          <span className="text-muted italic">{entries.length} keys</span>
        ) : (
          <>
            {entries.map(([key, val], i) => (
              <div key={key} style={{ paddingLeft: '1rem' }}>
                <span className={COLORS.key}>"{key}"</span>
                <span className={COLORS.punctuation}>: </span>
                <JsonValue value={val} depth={depth + 1} maxDepth={maxDepth} defaultCollapsed={defaultCollapsed} />
                {i < entries.length - 1 && <span className={COLORS.punctuation}>,</span>}
              </div>
            ))}
          </>
        )}
        <span className={COLORS.bracket}>{'}'}</span>
      </span>
    );
  }

  return <span className="text-muted">{String(value)}</span>;
}

export function JsonViewer({ 
  data, 
  collapsed = false, 
  depth = 0, 
  maxDepth = 5,
  className = ''
}: JsonViewerProps) {
  const [copied, setCopied] = useState(false);

  const copyToClipboard = useCallback(() => {
    const text = typeof data === 'string' ? data : JSON.stringify(data, null, 2);
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  }, [data]);

  return (
    <div className={`relative group ${className}`}>
      <button
        onClick={copyToClipboard}
        className="absolute right-2 top-2 p-1.5 rounded bg-dark-gray/80 border border-border opacity-0 group-hover:opacity-100 transition-opacity hover:bg-dark-gray"
        title="Copy JSON"
      >
        {copied ? (
          <Check className="w-3 h-3 text-success" />
        ) : (
          <Copy className="w-3 h-3 text-muted" />
        )}
      </button>
      <pre className="font-mono text-xs leading-relaxed overflow-x-auto">
        <JsonValue value={data} depth={depth} maxDepth={maxDepth} defaultCollapsed={collapsed} />
      </pre>
    </div>
  );
}

// Simple inline JSON with syntax highlighting (no collapsing)
export function JsonInline({ data, maxLength = 100 }: { data: unknown; maxLength?: number }) {
  const formatted = useMemo(() => {
    if (data === null) return <span className={COLORS.null}>null</span>;
    if (data === undefined) return <span className={COLORS.null}>undefined</span>;
    if (typeof data === 'string') {
      const display = data.length > maxLength ? `${data.slice(0, maxLength)}...` : data;
      return <span className={COLORS.string}>"{display}"</span>;
    }
    if (typeof data === 'number') return <span className={COLORS.number}>{data}</span>;
    if (typeof data === 'boolean') return <span className={COLORS.boolean}>{data ? 'true' : 'false'}</span>;
    
    const str = JSON.stringify(data);
    if (str.length > maxLength) {
      return <span className="text-muted">{str.slice(0, maxLength)}...</span>;
    }
    
    // Simple inline highlighting
    return (
      <span className="font-mono">
        {str.split(/(".*?":)|(".*?")|(\d+\.?\d*)|(\btrue\b|\bfalse\b)|(\bnull\b)/g).map((part, i) => {
          if (!part) return null;
          if (part.match(/^".*?":$/)) return <span key={i} className={COLORS.key}>{part}</span>;
          if (part.match(/^".*?"$/)) return <span key={i} className={COLORS.string}>{part}</span>;
          if (part.match(/^\d+\.?\d*$/)) return <span key={i} className={COLORS.number}>{part}</span>;
          if (part === 'true' || part === 'false') return <span key={i} className={COLORS.boolean}>{part}</span>;
          if (part === 'null') return <span key={i} className={COLORS.null}>{part}</span>;
          return <span key={i} className={COLORS.punctuation}>{part}</span>;
        })}
      </span>
    );
  }, [data, maxLength]);

  return <span className="font-mono text-xs">{formatted}</span>;
}
