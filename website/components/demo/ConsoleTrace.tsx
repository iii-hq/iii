import { useEffect, useRef, useState } from 'react';
import type { ConsoleTraceStep, SpanEntry, TraceEntry } from './types';

interface ConsoleTraceProps {
  step: ConsoleTraceStep;
  isActive: boolean;
  onNext: () => void;
}

type Phase = 'traces' | 'waterfall' | 'detail';

function TraceRow({
  trace,
  index,
  isSelected,
  visible,
}: {
  trace: TraceEntry;
  index: number;
  isSelected: boolean;
  visible: boolean;
}) {
  const isError = trace.status === 'error';
  return (
    <div
      className={`flex items-center gap-2 px-2.5 py-1 rounded text-[11px] font-mono transition-all duration-300 ${
        visible ? 'opacity-100 translate-x-0' : 'opacity-0 translate-x-2'
      } ${isSelected ? 'bg-iii-medium/10' : ''}`}
      style={{ transitionDelay: `${index * 80}ms` }}
    >
      <span
        className={`w-1.5 h-1.5 rounded-full shrink-0 ${
          isError ? 'bg-iii-alert' : 'bg-iii-success'
        }`}
      />
      <span
        className={`flex-1 truncate ${isError ? 'text-iii-alert' : 'text-iii-light/80'}`}
      >
        {trace.operation}
      </span>
      <span className="text-iii-medium/50 tabular-nums">
        {trace.duration ?? '---'}
      </span>
      <span
        className={`text-[10px] font-bold uppercase ${
          isError ? 'text-iii-alert' : 'text-iii-success/70'
        }`}
      >
        {trace.status === 'error' ? 'ERR' : 'OK'}
      </span>
    </div>
  );
}

function WaterfallRow({
  span,
  isError,
  visible,
  index,
}: {
  span: SpanEntry;
  isError: boolean;
  visible: boolean;
  index: number;
}) {
  const barColor = span.status === 'error' ? 'bg-iii-alert' : 'bg-iii-accent';
  const textColor =
    span.status === 'error' ? 'text-iii-alert' : 'text-iii-light/80';

  return (
    <div
      className={`flex items-center gap-2 py-0.5 text-[11px] font-mono transition-all duration-300 ${
        visible ? 'opacity-100 translate-x-0' : 'opacity-0 translate-x-2'
      }`}
      style={{
        paddingLeft: `${span.depth * 16 + 8}px`,
        transitionDelay: `${index * 120}ms`,
      }}
    >
      <span className={`shrink-0 text-[9px] ${textColor}`}>▸</span>
      <span className={`shrink-0 truncate max-w-[160px] ${textColor}`}>
        {span.label}
      </span>
      <div className="flex-1 flex items-center gap-1.5 min-w-0">
        <div className="flex-1 h-2.5 rounded-sm bg-iii-medium/10 overflow-hidden">
          <div
            className={`h-full rounded-sm transition-all duration-500 ${barColor} ${
              visible ? '' : 'w-0'
            }`}
            style={{
              width: visible ? `${span.widthPercent}%` : '0%',
              transitionDelay: `${index * 120 + 200}ms`,
            }}
          />
        </div>
        <span className="text-iii-medium/50 tabular-nums shrink-0">
          {span.duration}
        </span>
      </div>
      {span.status === 'error' && (
        <span className="text-[9px] font-bold text-iii-alert uppercase tracking-wider">
          ERROR
        </span>
      )}
    </div>
  );
}

function DetailPanel({
  detail,
  visible,
}: {
  detail: ConsoleTraceStep['detail'];
  visible: boolean;
}) {
  return (
    <div
      className={`space-y-1 text-[11px] font-mono transition-all duration-300 ${
        visible ? 'opacity-100 translate-y-0' : 'opacity-0 translate-y-2'
      }`}
    >
      <div className="flex gap-3">
        <span className="text-iii-medium/50 w-14 shrink-0">Status</span>
        <span className="text-iii-alert font-bold">{detail.status}</span>
      </div>
      <div className="flex gap-3">
        <span className="text-iii-medium/50 w-14 shrink-0">Service</span>
        <span className="text-iii-light/80">{detail.service}</span>
      </div>
      {detail.error && (
        <div className="flex gap-3">
          <span className="text-iii-medium/50 w-14 shrink-0">Error</span>
          <span className="text-iii-alert/90 break-all">{detail.error}</span>
        </div>
      )}
    </div>
  );
}

export function ConsoleTrace({ step, isActive, onNext }: ConsoleTraceProps) {
  const [phase, setPhase] = useState<Phase>('traces');
  const [tracesRevealed, setTracesRevealed] = useState(false);
  const [waterfallRevealed, setWaterfallRevealed] = useState(false);
  const [detailRevealed, setDetailRevealed] = useState(false);
  const [completed, setCompleted] = useState(false);
  const timersRef = useRef<ReturnType<typeof setTimeout>[]>([]);

  const clearTimers = () => {
    for (const t of timersRef.current) clearTimeout(t);
    timersRef.current = [];
  };

  useEffect(() => {
    if (!isActive) return;

    const traceDelay = step.traces.length * 80 + 400;
    const waterfallDelay = traceDelay + step.spans.length * 120 + 400;
    const detailDelay = waterfallDelay + 400;

    timersRef.current.push(setTimeout(() => setTracesRevealed(true), 100));

    timersRef.current.push(
      setTimeout(() => {
        setPhase('waterfall');
        setWaterfallRevealed(true);
      }, traceDelay),
    );

    timersRef.current.push(
      setTimeout(() => {
        setPhase('detail');
        setDetailRevealed(true);
      }, detailDelay),
    );

    return clearTimers;
  }, [isActive, step.traces.length, step.spans.length]);

  useEffect(() => {
    if (!isActive || !step.autoAdvance || !detailRevealed) return;
    const timer = setTimeout(onNext, step.autoAdvance);
    return () => clearTimeout(timer);
  }, [isActive, step.autoAdvance, onNext, detailRevealed]);

  const handleDone = () => {
    if (completed) return;
    setCompleted(true);
    onNext();
  };

  if (!isActive && completed) {
    const hasError = step.errorSpanIndex != null;
    return (
      <div
        className={`rounded-lg border px-3 py-2.5 ${
          hasError
            ? 'border-iii-alert/20 bg-iii-alert/5'
            : 'border-iii-success/20 bg-iii-success/5'
        }`}
      >
        <p
          className={`text-xs font-bold flex items-center gap-1.5 ${
            hasError ? 'text-iii-alert' : 'text-iii-success'
          }`}
        >
          <span>{hasError ? '◉' : '✓'}</span>
          <span>
            {hasError ? 'Error identified in traces' : 'Traces inspected'}
          </span>
        </p>
      </div>
    );
  }

  return (
    <div
      className={`rounded-lg border border-iii-medium/20 bg-iii-dark overflow-hidden ${
        isActive
          ? 'animate-[fadeSlideIn_0.3s_ease-out_forwards] opacity-0'
          : 'opacity-75'
      }`}
      style={isActive ? { animationFillMode: 'forwards' } : undefined}
    >
      {/* Header */}
      <div className="flex items-center justify-between px-3 py-1.5 bg-iii-black/50 border-b border-iii-medium/10">
        <div className="flex items-center gap-1.5">
          <span className="text-[11px] text-iii-accent">◉</span>
          <span className="text-[11px] text-iii-medium font-mono">
            iii console
          </span>
        </div>
        <div className="flex items-center gap-2">
          {(['traces', 'waterfall', 'detail'] as Phase[]).map((p) => (
            <span
              key={p}
              className={`text-[9px] uppercase tracking-wider font-bold ${
                phase === p ? 'text-iii-accent' : 'text-iii-medium/30'
              }`}
            >
              {p}
            </span>
          ))}
        </div>
      </div>

      {/* Trace list */}
      <div className="px-1.5 py-1.5 border-b border-iii-medium/10">
        {step.traces.map((trace: TraceEntry, i: number) => (
          <TraceRow
            key={i}
            trace={trace}
            index={i}
            isSelected={i === step.activeTraceIndex && phase !== 'traces'}
            visible={tracesRevealed}
          />
        ))}
      </div>

      {/* Waterfall */}
      {(phase === 'waterfall' || phase === 'detail') && (
        <div className="px-1.5 py-2 border-b border-iii-medium/10">
          {step.spans.map((span: SpanEntry, i: number) => (
            <WaterfallRow
              key={i}
              span={span}
              isError={step.errorSpanIndex === i}
              visible={waterfallRevealed}
              index={i}
            />
          ))}
        </div>
      )}

      {/* Detail */}
      {phase === 'detail' && (
        <div className="px-3 py-2.5">
          <DetailPanel detail={step.detail} visible={detailRevealed} />
        </div>
      )}

      {/* Action */}
      {isActive && detailRevealed && !step.autoAdvance && (
        <div className="flex justify-end px-3 py-2 border-t border-iii-medium/10">
          <button
            onClick={handleDone}
            className="px-4 py-1.5 text-xs font-bold rounded bg-iii-accent text-iii-black hover:brightness-110 transition-all duration-200 cursor-pointer"
          >
            Continue
          </button>
        </div>
      )}
    </div>
  );
}
