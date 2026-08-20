import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  type ChangeEvent,
  type ReactNode,
} from "react";
import type {
  ConfigFormHost,
  ConfigFormProps,
  HealthCheckResult,
  HealthComponent,
  JsonObject,
  JsonValue,
} from "./types";

const GIB = 1024 ** 3;
const MIB = 1024 ** 2;

const DEFAULT_TRACE_STORAGE: JsonObject = {
  enabled: true,
  directory: "./data/observability/traces",
  max_disk_bytes: GIB,
  retention_seconds: 2_592_000,
  memory_max_bytes: 256 * MIB,
  memory_low_watermark_ratio: 0.75,
  pending_max_age_seconds: 3_600,
};

interface UnitOption {
  factor: number;
  label: string;
}

const BYTE_UNITS: readonly UnitOption[] = [
  { factor: MIB, label: "MiB" },
  { factor: GIB, label: "GiB" },
];

const RETENTION_UNITS: readonly UnitOption[] = [
  { factor: 3_600, label: "hours" },
  { factor: 86_400, label: "days" },
];

const PENDING_UNITS: readonly UnitOption[] = [
  { factor: 60, label: "minutes" },
  { factor: 3_600, label: "hours" },
];

type SectionId = "overview" | "advanced";
type HealthState = "loading" | "ready" | "error";
type MeterTone = "good" | "warn" | "muted";

interface ObservabilityConfigFormProps extends ConfigFormProps {
  healthCheck: () => Promise<HealthCheckResult>;
}

interface FieldProps {
  label: string;
  description?: string;
  children: ReactNode;
  className?: string;
}

interface ToggleProps {
  checked: boolean;
  onChange(checked: boolean): void;
  disabled?: boolean;
  onLabel?: string;
  offLabel?: string;
}

interface NumberFieldProps {
  id: string;
  value: number | undefined;
  min?: number;
  max?: number;
  step?: number;
  suffix?: string;
  placeholder?: string;
  disabled?: boolean;
  onChange(value: number | undefined): void;
}

function isObject(value: JsonValue | undefined): value is JsonObject {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

function asObject(value: JsonValue | undefined): JsonObject {
  return isObject(value) ? value : {};
}

function asString(value: JsonValue | undefined, fallback = ""): string {
  return typeof value === "string" ? value : fallback;
}

function asNumber(value: JsonValue | undefined, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function asBoolean(value: JsonValue | undefined, fallback = false): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function detailsOf(component: HealthComponent | undefined): JsonObject {
  return asObject(component?.details);
}

function formatQuantity(value: number): string {
  const rounded = value >= 10 ? Math.round(value) : Math.round(value * 10) / 10;
  return rounded.toLocaleString("en-US");
}

function formatBytes(value: number | undefined): string {
  if (value === undefined || !Number.isFinite(value)) return "—";
  if (value >= GIB) return `${formatQuantity(value / GIB)} GiB`;
  if (value >= MIB) return `${formatQuantity(value / MIB)} MiB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KiB`;
  return `${Math.round(value)} B`;
}

function titleCase(value: string): string {
  return value
    .replace(/[_-]+/g, " ")
    .replace(/\b\w/g, (character) => character.toUpperCase());
}

function statusTone(value: string | undefined): "good" | "warn" | "bad" | "muted" {
  if (value === "healthy" || value === "complete") return "good";
  if (value === "degraded" || value === "partial" || value === "unknown") return "warn";
  if (value === "error" || value === "failed") return "bad";
  return "muted";
}

function updateRoot(
  props: ConfigFormProps,
  key: string,
  next: JsonValue | undefined,
): void {
  const root = { ...asObject(props.value) };
  if (next === undefined) delete root[key];
  else root[key] = next;
  props.onChange(root);
}

function updateObject(
  props: ConfigFormProps,
  parentKey: string,
  key: string,
  next: JsonValue | undefined,
): void {
  const root = { ...asObject(props.value) };
  const parent = { ...asObject(root[parentKey]) };
  if (next === undefined) delete parent[key];
  else parent[key] = next;
  root[parentKey] = parent;
  props.onChange(root);
}

function updateNestedObject(
  props: ConfigFormProps,
  firstKey: string,
  secondKey: string,
  key: string,
  next: JsonValue | undefined,
): void {
  const root = { ...asObject(props.value) };
  const first = { ...asObject(root[firstKey]) };
  const second = { ...asObject(first[secondKey]) };
  if (next === undefined) delete second[key];
  else second[key] = next;
  first[secondKey] = second;
  root[firstKey] = first;
  props.onChange(root);
}

function updateNestedArrayItem(
  props: ConfigFormProps,
  parentKey: string,
  key: string,
  index: number,
  update: (item: JsonObject) => JsonObject,
): void {
  const root = { ...asObject(props.value) };
  const parent = { ...asObject(root[parentKey]) };
  const items = Array.isArray(parent[key]) ? [...(parent[key] as JsonValue[])] : [];
  const item = { ...asObject(items[index]) };
  items[index] = update(item);
  parent[key] = items;
  root[parentKey] = parent;
  props.onChange(root);
}

function addNestedArrayItem(
  props: ConfigFormProps,
  parentKey: string,
  key: string,
  item: JsonObject,
): void {
  const root = { ...asObject(props.value) };
  const parent = { ...asObject(root[parentKey]) };
  const items = Array.isArray(parent[key]) ? [...(parent[key] as JsonValue[])] : [];
  items.push(item);
  parent[key] = items;
  root[parentKey] = parent;
  props.onChange(root);
}

function removeNestedArrayItem(props: ConfigFormProps, parentKey: string, key: string, index: number): void {
  const root = { ...asObject(props.value) };
  const parent = { ...asObject(root[parentKey]) };
  const items = Array.isArray(parent[key]) ? [...(parent[key] as JsonValue[])] : [];
  items.splice(index, 1);
  if (items.length === 0) delete parent[key];
  else parent[key] = items;
  root[parentKey] = parent;
  props.onChange(root);
}

function DatabaseIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" aria-hidden="true">
      <ellipse cx="8" cy="3.8" rx="5.3" ry="2.3" />
      <path d="M2.7 3.8v8.4c0 1.27 2.37 2.3 5.3 2.3s5.3-1.03 5.3-2.3V3.8" />
      <path d="M2.7 8c0 1.27 2.37 2.3 5.3 2.3S13.3 9.27 13.3 8" />
    </svg>
  );
}

function ChipIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" aria-hidden="true">
      <rect x="3.2" y="3.2" width="9.6" height="9.6" rx="1.5" />
      <rect x="6.1" y="6.1" width="3.8" height="3.8" rx="0.6" />
      <path d="M5.5 1v2.2M8 1v2.2M10.5 1v2.2M5.5 12.8V15M8 12.8V15M10.5 12.8V15M1 5.5h2.2M1 8h2.2M1 10.5h2.2M12.8 5.5H15M12.8 8H15M12.8 10.5H15" />
    </svg>
  );
}

function WarningIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth={1.4} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M8 2.2 14.6 13.4H1.4L8 2.2Z" />
      <path d="M8 6.5v3.2" />
      <path d="M8 11.7v0.01" />
    </svg>
  );
}

function Field({ label, description, children, className = "" }: FieldProps) {
  return (
    <div className={`obs-field ${className}`}>
      <div className="obs-field__label">{label}</div>
      {children}
      {description ? <p className="obs-field__description">{description}</p> : null}
    </div>
  );
}

function Toggle({
  checked,
  onChange,
  disabled = false,
  onLabel = "Enabled",
  offLabel = "Disabled",
}: ToggleProps) {
  return (
    <button
      type="button"
      className={`obs-toggle${checked ? " is-on" : ""}`}
      aria-pressed={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
    >
      <span className="obs-toggle__track">
        <span className="obs-toggle__thumb" />
      </span>
      <span>{checked ? onLabel : offLabel}</span>
    </button>
  );
}

function NumberField({
  id,
  value,
  min,
  max,
  step = 1,
  suffix,
  placeholder,
  disabled = false,
  onChange,
}: NumberFieldProps) {
  const handleChange = (event: ChangeEvent<HTMLInputElement>) => {
    const raw = event.target.value;
    onChange(raw === "" ? undefined : Number(raw));
  };

  return (
    <div className="obs-number-field">
      <input
        id={id}
        type="number"
        value={value ?? ""}
        min={min}
        max={max}
        step={step}
        placeholder={placeholder}
        disabled={disabled}
        onChange={handleChange}
      />
      {suffix ? <span className="obs-number-field__suffix">{suffix}</span> : null}
    </div>
  );
}

function pickUnitIndex(base: number | undefined, units: readonly UnitOption[]): number {
  if (base === undefined || base <= 0) return units.length - 1;
  for (let index = units.length - 1; index >= 0; index -= 1) {
    if (base % units[index].factor === 0) return index;
  }
  for (let index = units.length - 1; index >= 0; index -= 1) {
    if (base >= units[index].factor) return index;
  }
  return 0;
}

function formatUnitValue(base: number, factor: number): string {
  return String(Math.round((base / factor) * 100) / 100);
}

function UnitNumberField({
  id,
  baseValue,
  units,
  minBase = 0,
  disabled = false,
  onChange,
}: {
  id: string;
  baseValue: number | undefined;
  units: readonly UnitOption[];
  minBase?: number;
  disabled?: boolean;
  onChange(base: number | undefined): void;
}) {
  const [unitIndex, setUnitIndex] = useState(() => pickUnitIndex(baseValue, units));
  const unit = units[Math.min(unitIndex, units.length - 1)];
  const [text, setText] = useState(() =>
    baseValue === undefined ? "" : formatUnitValue(baseValue, unit.factor),
  );
  const [lastBase, setLastBase] = useState(baseValue);

  // Re-sync the visible text only on external value changes so typing stays free.
  if (baseValue !== lastBase) {
    setLastBase(baseValue);
    setText(baseValue === undefined ? "" : formatUnitValue(baseValue, unit.factor));
  }

  const emit = (nextBase: number | undefined) => {
    setLastBase(nextBase);
    onChange(nextBase);
  };

  const handleValueChange = (event: ChangeEvent<HTMLInputElement>) => {
    const raw = event.target.value;
    setText(raw);
    if (raw === "") {
      emit(undefined);
      return;
    }
    const parsed = Number(raw);
    if (!Number.isFinite(parsed) || parsed < 0) return;
    emit(Math.max(minBase, Math.round(parsed * unit.factor)));
  };

  const handleBlur = () => {
    setText(baseValue === undefined ? "" : formatUnitValue(baseValue, unit.factor));
  };

  const handleUnitChange = (event: ChangeEvent<HTMLSelectElement>) => {
    const nextIndex = Number(event.target.value);
    const nextUnit = units[nextIndex] ?? unit;
    setUnitIndex(nextIndex);
    setText(baseValue === undefined ? "" : formatUnitValue(baseValue, nextUnit.factor));
  };

  return (
    <div className="obs-unit-field">
      <input
        id={id}
        type="number"
        min={0}
        value={text}
        disabled={disabled}
        onChange={handleValueChange}
        onBlur={handleBlur}
      />
      <select
        aria-label="Unit"
        value={String(Math.min(unitIndex, units.length - 1))}
        disabled={disabled}
        onChange={handleUnitChange}
      >
        {units.map((option, index) => (
          <option key={option.label} value={String(index)}>
            {option.label}
          </option>
        ))}
      </select>
    </div>
  );
}

function SelectField({
  id,
  value,
  options,
  disabled = false,
  onChange,
}: {
  id: string;
  value: string | undefined;
  options: readonly { value: string; label: string }[];
  disabled?: boolean;
  onChange(value: string | undefined): void;
}) {
  return (
    <select
      id={id}
      value={value ?? ""}
      disabled={disabled}
      onChange={(event) => onChange(event.target.value || undefined)}
    >
      <option value="">Use default</option>
      {options.map((option) => (
        <option key={option.value} value={option.value}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

function TextField({
  id,
  value,
  placeholder,
  disabled = false,
  templateHint = false,
  onChange,
}: {
  id: string;
  value: string | undefined;
  placeholder?: string;
  disabled?: boolean;
  templateHint?: boolean;
  onChange(value: string | undefined): void;
}) {
  const [focused, setFocused] = useState(false);
  const raw = value ?? "";
  // `${VAR:default}` templates are expanded by the configuration worker at
  // read time; render them in a lighter tone so they read as variables, not
  // literal values.
  const isTemplate = /\$\{[^}]*\}/.test(raw);
  // With templateHint, a `${VAR:default}` value shows only `${VAR}` while the
  // default moves to a hint line below; the full value comes back on focus so
  // edits always operate on what is actually stored.
  const template = templateHint ? /^\$\{([A-Za-z0-9_]+):(.*)\}$/.exec(raw) : null;
  const display = template && !focused ? `\${${template[1]}}` : raw;

  const input = (
    <input
      id={id}
      type="text"
      className={isTemplate ? "is-template" : undefined}
      value={display}
      placeholder={placeholder}
      disabled={disabled}
      onFocus={() => setFocused(true)}
      onBlur={() => setFocused(false)}
      onChange={(event) => onChange(event.target.value || undefined)}
    />
  );

  if (!templateHint) return input;
  return (
    <>
      {input}
      {template ? <span className="obs-field__default">default: {template[2]}</span> : null}
    </>
  );
}

function StatusPill({
  health,
  state,
}: {
  health: HealthCheckResult | null;
  state: HealthState;
}) {
  let tone: "good" | "warn" | "bad" | "muted";
  let text: string;

  if (state === "error") {
    tone = "bad";
    text = "engine unavailable";
  } else if (!health) {
    tone = "muted";
    text = "updating…";
  } else {
    const status = asString(health.status, "unknown");
    tone = statusTone(status);
    if (status === "healthy") text = "healthy pipeline · updates every 5 s";
    else if (status === "degraded") text = "degraded pipeline · updates every 5 s";
    else if (status === "error" || status === "failed") text = "pipeline failure";
    else text = `pipeline: ${titleCase(status)}`;
  }

  return (
    <span className={`obs-status-pill obs-status-pill--${tone}`} role="status">
      <span className={`obs-status-dot obs-status-dot--${tone}`} />
      <span>{text}</span>
    </span>
  );
}

interface UsageMeterProps {
  label: string;
  used: number | undefined;
  limit: number | undefined;
  caption: string;
  tone: MeterTone;
}

function UsageMeter({ label, used, limit, caption, tone }: UsageMeterProps) {
  const pct =
    used !== undefined && limit !== undefined && limit > 0
      ? Math.min(100, Math.round((used / limit) * 100))
      : 0;

  return (
    <div className="obs-meter">
      <div className="obs-meter__top">
        <span className="obs-meter__label">{label}</span>
        <span className="obs-meter__value">
          {formatBytes(used)} <span>de {formatBytes(limit)}</span>
        </span>
      </div>
      <div className="obs-meter__track">
        <div
          className={`obs-meter__fill${pct >= 90 ? " obs-meter__fill--warn" : ""}`}
          style={{ width: `${pct}%` }}
        />
      </div>
      <div className="obs-meter__caption">
        <span className={`obs-status-dot obs-status-dot--${tone}`} />
        <span>{caption}</span>
      </div>
    </div>
  );
}

function PersistenceSection({
  props,
  health,
}: {
  props: ConfigFormProps;
  health: HealthCheckResult | null;
}) {
  const rawStorage = asObject(asObject(props.value).trace_storage);
  const storage = { ...DEFAULT_TRACE_STORAGE, ...rawStorage };
  const enabled = asBoolean(storage.enabled, true);
  const maxDiskBytes = asNumber(storage.max_disk_bytes);
  const memoryMaxBytes = asNumber(storage.memory_max_bytes);

  const spanDetails = detailsOf(health?.components?.spans);
  const archive = asObject(spanDetails.archive);
  const droppedSpans = asNumber(archive.known_dropped_spans, 0);

  const hasData = health !== null;
  let caption = "waiting for engine data…";
  let tone: MeterTone = "muted";
  if (hasData) {
    if (droppedSpans > 0) {
      caption = `${droppedSpans.toLocaleString("en-US")} spans dropped`;
      tone = "warn";
    } else {
      caption = "traces.db + WAL · no trace loss";
      tone = "good";
    }
  }

  return (
    <section className="obs-card obs-focus-card">
      <div className="obs-card__header">
        <div className="obs-card__heading">
          <span className={`obs-card__icon${enabled ? "" : " obs-card__icon--muted"}`}>
            <DatabaseIcon />
          </span>
          <div className="obs-card__titles">
            <h3>Disk persistence</h3>
            <p className="obs-card__description">Trace history stored in a local SQLite database.</p>
          </div>
        </div>
        <Toggle
          checked={enabled}
          onLabel="Enabled"
          offLabel="Disabled"
          onChange={(next) => updateObject(props, "trace_storage", "enabled", next)}
        />
      </div>

      {enabled ? (
        <UsageMeter
          label="Storage used"
          used={hasData ? asNumber(archive.physical_bytes, 0) : undefined}
          limit={maxDiskBytes}
          caption={caption}
          tone={tone}
        />
      ) : (
        <div className="obs-banner obs-banner--warn">
          <WarningIcon />
          <div>
            <strong>Memory only — history is lost when the engine restarts.</strong>
            <span>
              Without persistence, traces exist only in the cache: once it reaches the{" "}
              {formatBytes(memoryMaxBytes)} limit, the oldest traces are permanently discarded.
            </span>
          </div>
        </div>
      )}

      <Field
        label="Storage limit"
        description="Maximum directory size. Once the limit is reached, the oldest traces are deleted first. Minimum: 64 MiB."
      >
        <UnitNumberField
          id="trace-storage-max-disk"
          baseValue={maxDiskBytes}
          units={BYTE_UNITS}
          minBase={64 * MIB}
          disabled={!enabled}
          onChange={(value) => updateObject(props, "trace_storage", "max_disk_bytes", value)}
        />
      </Field>
      <Field
        label="Retain traces for"
        description="Traces older than this are deleted even when space is available. Use 0 to retain them until the storage limit is reached."
      >
        <UnitNumberField
          id="trace-storage-retention"
          baseValue={asNumber(storage.retention_seconds)}
          units={RETENTION_UNITS}
          disabled={!enabled}
          onChange={(value) => updateObject(props, "trace_storage", "retention_seconds", value)}
        />
      </Field>
      <Field
        label="Data directory"
        description="Directory reserved for the database and WAL files. Relative paths start from the engine directory."
      >
        <TextField
          id="trace-storage-directory"
          value={asString(storage.directory)}
          disabled={!enabled}
          onChange={(value) => updateObject(props, "trace_storage", "directory", value)}
        />
      </Field>
    </section>
  );
}

function MemorySection({
  props,
  health,
}: {
  props: ConfigFormProps;
  health: HealthCheckResult | null;
}) {
  const root = asObject(props.value);
  const rawStorage = asObject(root.trace_storage);
  const storage = { ...DEFAULT_TRACE_STORAGE, ...rawStorage };
  const persistenceEnabled = asBoolean(storage.enabled, true);
  const memoryMaxBytes = asNumber(storage.memory_max_bytes);
  const watermarkRatio = asNumber(storage.memory_low_watermark_ratio, 0.75);

  const spanDetails = detailsOf(health?.components?.spans);
  const hasData = health !== null;
  const hotBytes = hasData ? asNumber(spanDetails.hot_bytes, 0) : undefined;
  const storedSpans = asNumber(spanDetails.stored_spans, 0);
  const pct = hotBytes !== undefined && memoryMaxBytes > 0 ? (hotBytes / memoryMaxBytes) * 100 : 0;

  let caption = "waiting for engine data…";
  let tone: MeterTone = "muted";
  if (hasData) {
    const spansLabel = `${storedSpans.toLocaleString("en-US")} spans in memory`;
    if (pct >= 90) {
      caption = persistenceEnabled
        ? `${spansLabel} · near the limit`
        : `${spansLabel} · near the limit, older traces will be discarded`;
      tone = "warn";
    } else {
      caption = spansLabel;
      tone = "good";
    }
  }

  return (
    <section className="obs-card obs-focus-card">
      <div className="obs-card__header">
        <div className="obs-card__heading">
          <span className="obs-card__icon">
            <ChipIcon />
          </span>
          <div className="obs-card__titles">
            <h3>Memory usage</h3>
            <p className="obs-card__description">
              {persistenceEnabled
                ? "Recent traces stay in RAM for immediate queries."
                : "With persistence disabled, the cache is the only storage."}
            </p>
          </div>
        </div>
      </div>

      <UsageMeter
        label="Cache used"
        used={hotBytes}
        limit={memoryMaxBytes}
        caption={caption}
        tone={tone}
      />

      <Field
        label="Cache limit"
        description={
          persistenceEnabled
            ? "Above this limit, older traces leave memory and are read from disk instead. Minimum: 16 MiB."
            : "Above this limit, older traces leave memory. Without persistence, they are lost. Minimum: 16 MiB."
        }
      >
        <NumberField
          id="trace-storage-memory"
          value={Math.round(memoryMaxBytes / MIB)}
          min={16}
          step={16}
          suffix="MiB"
          onChange={(value) =>
            updateObject(
              props,
              "trace_storage",
              "memory_max_bytes",
              value === undefined ? undefined : Math.max(16, value) * MIB,
            )
          }
        />
      </Field>
      <Field
        label="Keep after cleanup"
        description="Each cleanup frees memory until usage falls to this fraction of the limit. Between 50% and 95%."
      >
        <NumberField
          id="trace-storage-low-watermark"
          value={Math.round(watermarkRatio * 100)}
          min={50}
          max={95}
          step={5}
          suffix="% do limite"
          onChange={(value) =>
            updateObject(
              props,
              "trace_storage",
              "memory_low_watermark_ratio",
              value === undefined ? undefined : Math.min(95, Math.max(50, value)) / 100,
            )
          }
        />
      </Field>
      <Field
        label="Discard incomplete trace after"
        description="A trace that never receives its final span is closed and released from memory after this time."
      >
        <UnitNumberField
          id="trace-storage-pending-age"
          baseValue={asNumber(storage.pending_max_age_seconds)}
          units={PENDING_UNITS}
          onChange={(value) =>
            updateObject(props, "trace_storage", "pending_max_age_seconds", value)
          }
        />
      </Field>
      <Field
        label="Spans retained by exporter"
        description="Applies to the Memory and Memory + OTLP exporters. Empty uses the engine default."
        className="obs-field--separated"
      >
        <NumberField
          id="pipeline-memory-spans"
          value={root.memory_max_spans === undefined ? undefined : asNumber(root.memory_max_spans)}
          min={1}
          step={1_000}
          suffix="spans"
          placeholder="engine default"
          onChange={(value) => updateRoot(props, "memory_max_spans", value)}
        />
      </Field>
    </section>
  );
}

function PipelineSection({ props }: { props: ConfigFormProps }) {
  const root = asObject(props.value);
  const enabled = asBoolean(root.enabled, true);
  const exporter = asString(root.exporter);

  return (
    <section className="obs-card">
      <div className="obs-card__header">
        <div className="obs-card__titles">
          <h3>Collection and export</h3>
          <p className="obs-card__description">How the engine collects, samples, and forwards telemetry.</p>
        </div>
        <Toggle
          checked={enabled}
          onLabel="Enabled"
          offLabel="Disabled"
          onChange={(next) => updateRoot(props, "enabled", next)}
        />
      </div>
      <div className="obs-form-grid obs-form-grid--three">
        <Field label="Exporter" description="Where completed spans are sent.">
          <SelectField
            id="pipeline-exporter"
            value={exporter || undefined}
            options={[
              { value: "memory", label: "Memory" },
              { value: "otlp", label: "OTLP" },
              { value: "both", label: "Memory + OTLP" },
            ]}
            onChange={(value) => updateRoot(props, "exporter", value)}
          />
        </Field>
        <Field label="Service name" description="Identifies this engine in exported traces.">
          <TextField
            id="pipeline-service-name"
            value={asString(root.service_name)}
            placeholder="iii"
            onChange={(value) => updateRoot(props, "service_name", value)}
          />
        </Field>
        <Field label="Sampling" description="1.0 keeps all traces; 0.1 keeps 1 in 10.">
          <NumberField
            id="pipeline-sampling-ratio"
            value={root.sampling_ratio === undefined ? undefined : asNumber(root.sampling_ratio)}
            min={0}
            max={1}
            step={0.05}
            suffix="ratio"
            onChange={(value) => updateRoot(props, "sampling_ratio", value)}
          />
        </Field>
        <Field label="OTLP endpoint" description="Used only with the OTLP or Memory + OTLP exporter.">
          <TextField
            id="pipeline-endpoint"
            value={asString(root.endpoint)}
            placeholder="http://localhost:4317"
            onChange={(value) => updateRoot(props, "endpoint", value)}
          />
        </Field>
        <Field label="Live spans" description="Includes in-progress traces in Console queries.">
          <Toggle
            checked={asBoolean(root.live_spans)}
            onLabel="Enabled"
            offLabel="Disabled"
            onChange={(next) => updateRoot(props, "live_spans", next)}
          />
        </Field>
      </div>
    </section>
  );
}

function SamplingSection({ props }: { props: ConfigFormProps }) {
  const sampling = asObject(asObject(props.value).sampling);
  const rules = Array.isArray(sampling.rules) ? sampling.rules : [];
  const rateLimit = asObject(sampling.rate_limit);

  return (
    <section className="obs-card">
      <div className="obs-card__header">
        <div>
          <p className="obs-kicker">Advanced · Sampling</p>
          <h3>Sampling rules</h3>
          <p className="obs-card__description">Prioritize important operations without losing control of volume.</p>
        </div>
      </div>
      <div className="obs-form-grid obs-form-grid--three">
        <Field label="Default ratio">
          <NumberField
            id="sampling-default"
            value={sampling.default === undefined ? undefined : asNumber(sampling.default)}
            min={0}
            max={1}
            step={0.05}
            suffix="ratio"
            onChange={(value) => updateObject(props, "sampling", "default", value)}
          />
        </Field>
        <Field label="Parent-based" description="Inherit the decision from the parent span.">
          <Toggle
            checked={asBoolean(sampling.parent_based)}
            onChange={(next) => updateObject(props, "sampling", "parent_based", next)}
          />
        </Field>
        <Field label="Rate limit">
          <NumberField
            id="sampling-rate-limit"
            value={rateLimit.max_traces_per_second === undefined ? undefined : asNumber(rateLimit.max_traces_per_second)}
            min={1}
            step={10}
            suffix="traces/s"
            onChange={(value) => updateNestedObject(props, "sampling", "rate_limit", "max_traces_per_second", value)}
          />
        </Field>
      </div>

      <div className="obs-subsection">
        <div className="obs-subsection__header">
          <div>
            <strong>Rules by operation</strong>
            <span>Applied in the order shown.</span>
          </div>
          <button
            type="button"
            className="obs-button obs-button--secondary"
            onClick={() => addNestedArrayItem(props, "sampling", "rules", { rate: 0.1 })}
          >
            Add rule
          </button>
        </div>
        {rules.length === 0 ? (
          <div className="obs-empty">No specific rules. The default ratio will be used.</div>
        ) : (
          <div className="obs-rules">
            {rules.map((rawRule, index) => {
              const rule = asObject(rawRule);
              return (
                <div className="obs-rule" key={`sampling-rule-${index}`}>
                  <span className="obs-rule__index">{index + 1}</span>
                  <TextField
                    id={`sampling-operation-${index}`}
                    value={asString(rule.operation)}
                    placeholder="operation, ex.: checkout.*"
                    onChange={(value) => updateNestedArrayItem(props, "sampling", "rules", index, (item) => {
                      if (value === undefined) delete item.operation;
                      else item.operation = value;
                      return item;
                    })}
                  />
                  <TextField
                    id={`sampling-service-${index}`}
                    value={asString(rule.service)}
                    placeholder="service (opcional)"
                    onChange={(value) => updateNestedArrayItem(props, "sampling", "rules", index, (item) => {
                      if (value === undefined) delete item.service;
                      else item.service = value;
                      return item;
                    })}
                  />
                  <NumberField
                    id={`sampling-rate-${index}`}
                    value={asNumber(rule.rate, 0.1)}
                    min={0}
                    max={1}
                    step={0.05}
                    suffix="ratio"
                    onChange={(value) => updateNestedArrayItem(props, "sampling", "rules", index, (item) => ({
                      ...item,
                      rate: value ?? 0,
                    }))}
                  />
                  <button
                    type="button"
                    className="obs-icon-button"
                    aria-label={`Remover regra ${index + 1}`}
                    onClick={() => removeNestedArrayItem(props, "sampling", "rules", index)}
                  >
                    ×
                  </button>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </section>
  );
}

function MetricsLogsSection({ props }: { props: ConfigFormProps }) {
  const root = asObject(props.value);

  return (
    <section className="obs-card">
      <div className="obs-card__header">
        <div>
          <p className="obs-kicker">Advanced · Signals</p>
          <h3>Metrics and logs</h3>
          <p className="obs-card__description">Adjust retention and export without changing traces.</p>
        </div>
      </div>
      <div className="obs-signal-grid">
        <div className="obs-signal-panel">
          <div className="obs-signal-panel__header">
            <div><strong>Metrics</strong><span>Aggregated engine metrics.</span></div>
            <Toggle
              checked={asBoolean(root.metrics_enabled)}
              onChange={(next) => updateRoot(props, "metrics_enabled", next)}
            />
          </div>
          <Field label="Exporter">
            <SelectField
              id="metrics-exporter"
              value={asString(root.metrics_exporter) || undefined}
              options={[{ value: "memory", label: "Memory" }, { value: "otlp", label: "OTLP" }]}
              onChange={(value) => updateRoot(props, "metrics_exporter", value)}
            />
          </Field>
          <Field label="Max count">
            <NumberField
              id="metrics-max-count"
              value={root.metrics_max_count === undefined ? undefined : asNumber(root.metrics_max_count)}
              min={1}
              step={1000}
              suffix="items"
              onChange={(value) => updateRoot(props, "metrics_max_count", value)}
            />
          </Field>
          <Field label="Retention">
            <NumberField
              id="metrics-retention"
              value={root.metrics_retention_seconds === undefined ? undefined : asNumber(root.metrics_retention_seconds)}
              min={1}
              step={60}
              suffix="seconds"
              onChange={(value) => updateRoot(props, "metrics_retention_seconds", value)}
            />
          </Field>
        </div>
        <div className="obs-signal-panel">
          <div className="obs-signal-panel__header">
            <div><strong>Logs</strong><span>Structured events and engine output.</span></div>
            <Toggle checked={asBoolean(root.logs_enabled, true)} onChange={(next) => updateRoot(props, "logs_enabled", next)} />
          </div>
          <Field label="Exporter">
            <SelectField
              id="logs-exporter"
              value={asString(root.logs_exporter) || undefined}
              options={[{ value: "memory", label: "Memory" }, { value: "otlp", label: "OTLP" }, { value: "both", label: "Memory + OTLP" }]}
              onChange={(value) => updateRoot(props, "logs_exporter", value)}
            />
          </Field>
          <Field label="Sampling">
            <NumberField
              id="logs-sampling-ratio"
              value={asNumber(root.logs_sampling_ratio, 1)}
              min={0}
              max={1}
              step={0.05}
              suffix="ratio"
              onChange={(value) => updateRoot(props, "logs_sampling_ratio", value)}
            />
          </Field>
          <Field label="Console output">
            <Toggle checked={asBoolean(root.logs_console_output, true)} onChange={(next) => updateRoot(props, "logs_console_output", next)} />
          </Field>
        </div>
      </div>
    </section>
  );
}

function TextJsonField({
  id,
  label,
  value,
  onChange,
}: {
  id: string;
  label: string;
  value: JsonValue | undefined;
  onChange(value: JsonValue | undefined): void;
}) {
  const [text, setText] = useState(value === undefined ? "" : JSON.stringify(value, null, 2));
  const [invalid, setInvalid] = useState(false);

  useEffect(() => {
    setText(value === undefined ? "" : JSON.stringify(value, null, 2));
  }, [value]);

  const handleBlur = () => {
    if (text.trim() === "") {
      setInvalid(false);
      onChange(undefined);
      return;
    }
    try {
      const parsed = JSON.parse(text) as JsonValue;
      setInvalid(false);
      onChange(parsed);
    } catch {
      setInvalid(true);
    }
  };

  return (
    <Field label={label} description="Advanced JSON; unedited fields remain preserved.">
      <textarea
        id={id}
        className={invalid ? "is-invalid" : ""}
        value={text}
        rows={7}
        spellCheck={false}
        onChange={(event) => setText(event.target.value)}
        onBlur={handleBlur}
      />
      {invalid ? <span className="obs-field__error">Invalid JSON. Fix it before leaving this field.</span> : null}
    </Field>
  );
}

function AdvancedSections({ props }: { props: ConfigFormProps }) {
  const root = asObject(props.value);
  return (
    <div className="obs-stack">
      <SamplingSection props={props} />
      <MetricsLogsSection props={props} />
      <section className="obs-card">
        <div className="obs-card__header">
          <div>
            <p className="obs-kicker">Advanced · Runtime</p>
            <h3>Runtime and rules</h3>
            <p className="obs-card__description">Less frequent options are separated to keep the overview operational.</p>
          </div>
        </div>
        <div className="obs-form-grid obs-form-grid--three">
          <Field label="Service version">
            <TextField
              id="runtime-service-version"
              value={asString(root.service_version)}
              templateHint
              onChange={(value) => updateRoot(props, "service_version", value)}
            />
          </Field>
          <Field label="Service namespace">
            <TextField id="runtime-service-namespace" value={asString(root.service_namespace)} onChange={(value) => updateRoot(props, "service_namespace", value)} />
          </Field>
          <Field label="Log level">
            <SelectField
              id="runtime-level"
              value={asString(root.level) || undefined}
              options={["trace", "debug", "info", "warn", "error"].map((value) => ({ value, label: titleCase(value) }))}
              onChange={(value) => updateRoot(props, "level", value)}
            />
          </Field>
          <Field label="Log format">
            <SelectField
              id="runtime-format"
              value={asString(root.format) || undefined}
              options={[{ value: "default", label: "Human readable" }, { value: "json", label: "JSON" }]}
              onChange={(value) => updateRoot(props, "format", value)}
            />
          </Field>
          <Field label="Logs max count">
            <NumberField
              id="logs-max-count"
              value={root.logs_max_count === undefined ? undefined : asNumber(root.logs_max_count)}
              min={1}
              step={100}
              suffix="items"
              onChange={(value) => updateRoot(props, "logs_max_count", value)}
            />
          </Field>
          <Field label="Logs retention">
            <NumberField
              id="logs-retention"
              value={root.logs_retention_seconds === undefined ? undefined : asNumber(root.logs_retention_seconds)}
              min={1}
              step={60}
              suffix="seconds"
              onChange={(value) => updateRoot(props, "logs_retention_seconds", value)}
            />
          </Field>
        </div>
        <div className="obs-advanced-json">
          <TextJsonField id="alerts-json" label="Alert rules" value={root.alerts} onChange={(value) => updateRoot(props, "alerts", value)} />
          <TextJsonField id="collapse-spans-json" label="Span collapse rules" value={root.collapse_spans} onChange={(value) => updateRoot(props, "collapse_spans", value)} />
        </div>
      </section>
    </div>
  );
}

function ErrorSummary({ errors }: { errors?: ReadonlyMap<string, string> }) {
  const entries = useMemo(() => (errors ? Array.from(errors.entries()) : []), [errors]);
  if (entries.length === 0) return null;
  return (
    <div className="obs-error-summary" role="alert">
      <strong>Review the fields before saving</strong>
      {entries.map(([path, message]) => <span key={`${path}:${message}`}>{path || "configuration"}: {message}</span>)}
    </div>
  );
}

export function ObservabilityConfigPage({ healthCheck, ...props }: ObservabilityConfigFormProps) {
  const [section, setSection] = useState<SectionId>("overview");
  const [health, setHealth] = useState<HealthCheckResult | null>(null);
  const [healthState, setHealthState] = useState<HealthState>("loading");

  const refreshHealth = useCallback(async () => {
    setHealthState("loading");
    try {
      const result = await healthCheck();
      setHealth(result);
      setHealthState("ready");
    } catch {
      setHealthState("error");
    }
  }, [healthCheck]);

  useEffect(() => {
    let active = true;
    let inFlight = false;

    const refresh = async () => {
      if (!active || inFlight || document.visibilityState !== "visible") return;
      inFlight = true;
      await refreshHealth();
      inFlight = false;
    };

    void refresh();
    const timer = window.setInterval(() => void refresh(), 5_000);
    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", onVisibilityChange);

    return () => {
      active = false;
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [refreshHealth]);

  return (
    <div className="iii-observability-ui">
      <div className="obs-toolbar">
        <div className="obs-tabs" role="tablist" aria-label="Configuration sections">
          {(["overview", "advanced"] as const).map((id) => (
            <button
              type="button"
              role="tab"
              aria-selected={section === id}
              className={section === id ? "is-active" : ""}
              key={id}
              onClick={() => setSection(id)}
            >
              {id === "overview" ? "Overview" : "Advanced"}
            </button>
          ))}
        </div>
        <StatusPill health={health} state={healthState} />
      </div>

      <main className="obs-content" role="tabpanel">
        {section === "overview" ? (
          <div className="obs-stack">
            <div className="obs-focus-grid">
              <PersistenceSection props={props} health={health} />
              <MemorySection props={props} health={health} />
            </div>
            <PipelineSection props={props} />
          </div>
        ) : (
          <AdvancedSections props={props} />
        )}
      </main>

      <ErrorSummary errors={props.errors} />
    </div>
  );
}

export default function setup(host: ConfigFormHost): () => void {
  const healthCheck = () => host.iii.trigger<HealthCheckResult>("engine::health::check", {});
  const RegisteredForm = (props: ConfigFormProps) => (
    <ObservabilityConfigPage
      {...props}
      healthCheck={healthCheck}
    />
  );

  return host.configForms.register("iii-observability", RegisteredForm, { layout: "full" });
}
