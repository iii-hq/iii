// page.tsx
import {
  useCallback,
  useEffect,
  useMemo,
  useState
} from "react";
import { Fragment, jsx, jsxs } from "react/jsx-runtime";
var GIB = 1024 ** 3;
var MIB = 1024 ** 2;
var DEFAULT_TRACE_STORAGE = {
  enabled: true,
  directory: "./data/observability/traces",
  max_disk_bytes: GIB,
  retention_seconds: 2592e3,
  memory_max_bytes: 256 * MIB,
  memory_low_watermark_ratio: 0.75,
  pending_max_age_seconds: 3600
};
var BYTE_UNITS = [
  { factor: MIB, label: "MiB" },
  { factor: GIB, label: "GiB" }
];
var RETENTION_UNITS = [
  { factor: 3600, label: "hours" },
  { factor: 86400, label: "days" }
];
var PENDING_UNITS = [
  { factor: 60, label: "minutes" },
  { factor: 3600, label: "hours" }
];
function isObject(value) {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}
function asObject(value) {
  return isObject(value) ? value : {};
}
function asString(value, fallback = "") {
  return typeof value === "string" ? value : fallback;
}
function asNumber(value, fallback = 0) {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}
function asBoolean(value, fallback = false) {
  return typeof value === "boolean" ? value : fallback;
}
function detailsOf(component) {
  return asObject(component?.details);
}
function formatQuantity(value) {
  const rounded = value >= 10 ? Math.round(value) : Math.round(value * 10) / 10;
  return rounded.toLocaleString("en-US");
}
function formatBytes(value) {
  if (value === void 0 || !Number.isFinite(value)) return "\u2014";
  if (value >= GIB) return `${formatQuantity(value / GIB)} GiB`;
  if (value >= MIB) return `${formatQuantity(value / MIB)} MiB`;
  if (value >= 1024) return `${Math.round(value / 1024)} KiB`;
  return `${Math.round(value)} B`;
}
function titleCase(value) {
  return value.replace(/[_-]+/g, " ").replace(/\b\w/g, (character) => character.toUpperCase());
}
function statusTone(value) {
  if (value === "healthy" || value === "complete") return "good";
  if (value === "degraded" || value === "partial" || value === "unknown") return "warn";
  if (value === "error" || value === "failed") return "bad";
  return "muted";
}
function updateRoot(props, key, next) {
  const root = { ...asObject(props.value) };
  if (next === void 0) delete root[key];
  else root[key] = next;
  props.onChange(root);
}
function updateObject(props, parentKey, key, next) {
  const root = { ...asObject(props.value) };
  const parent = { ...asObject(root[parentKey]) };
  if (next === void 0) delete parent[key];
  else parent[key] = next;
  root[parentKey] = parent;
  props.onChange(root);
}
function updateNestedObject(props, firstKey, secondKey, key, next) {
  const root = { ...asObject(props.value) };
  const first = { ...asObject(root[firstKey]) };
  const second = { ...asObject(first[secondKey]) };
  if (next === void 0) delete second[key];
  else second[key] = next;
  first[secondKey] = second;
  root[firstKey] = first;
  props.onChange(root);
}
function updateNestedArrayItem(props, parentKey, key, index, update) {
  const root = { ...asObject(props.value) };
  const parent = { ...asObject(root[parentKey]) };
  const items = Array.isArray(parent[key]) ? [...parent[key]] : [];
  const item = { ...asObject(items[index]) };
  items[index] = update(item);
  parent[key] = items;
  root[parentKey] = parent;
  props.onChange(root);
}
function addNestedArrayItem(props, parentKey, key, item) {
  const root = { ...asObject(props.value) };
  const parent = { ...asObject(root[parentKey]) };
  const items = Array.isArray(parent[key]) ? [...parent[key]] : [];
  items.push(item);
  parent[key] = items;
  root[parentKey] = parent;
  props.onChange(root);
}
function removeNestedArrayItem(props, parentKey, key, index) {
  const root = { ...asObject(props.value) };
  const parent = { ...asObject(root[parentKey]) };
  const items = Array.isArray(parent[key]) ? [...parent[key]] : [];
  items.splice(index, 1);
  if (items.length === 0) delete parent[key];
  else parent[key] = items;
  root[parentKey] = parent;
  props.onChange(root);
}
function DatabaseIcon() {
  return /* @__PURE__ */ jsxs("svg", { width: "16", height: "16", viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round", "aria-hidden": "true", children: [
    /* @__PURE__ */ jsx("ellipse", { cx: "8", cy: "3.8", rx: "5.3", ry: "2.3" }),
    /* @__PURE__ */ jsx("path", { d: "M2.7 3.8v8.4c0 1.27 2.37 2.3 5.3 2.3s5.3-1.03 5.3-2.3V3.8" }),
    /* @__PURE__ */ jsx("path", { d: "M2.7 8c0 1.27 2.37 2.3 5.3 2.3S13.3 9.27 13.3 8" })
  ] });
}
function ChipIcon() {
  return /* @__PURE__ */ jsxs("svg", { width: "16", height: "16", viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round", "aria-hidden": "true", children: [
    /* @__PURE__ */ jsx("rect", { x: "3.2", y: "3.2", width: "9.6", height: "9.6", rx: "1.5" }),
    /* @__PURE__ */ jsx("rect", { x: "6.1", y: "6.1", width: "3.8", height: "3.8", rx: "0.6" }),
    /* @__PURE__ */ jsx("path", { d: "M5.5 1v2.2M8 1v2.2M10.5 1v2.2M5.5 12.8V15M8 12.8V15M10.5 12.8V15M1 5.5h2.2M1 8h2.2M1 10.5h2.2M12.8 5.5H15M12.8 8H15M12.8 10.5H15" })
  ] });
}
function WarningIcon() {
  return /* @__PURE__ */ jsxs("svg", { width: "16", height: "16", viewBox: "0 0 16 16", fill: "none", stroke: "currentColor", strokeWidth: 1.4, strokeLinecap: "round", strokeLinejoin: "round", "aria-hidden": "true", children: [
    /* @__PURE__ */ jsx("path", { d: "M8 2.2 14.6 13.4H1.4L8 2.2Z" }),
    /* @__PURE__ */ jsx("path", { d: "M8 6.5v3.2" }),
    /* @__PURE__ */ jsx("path", { d: "M8 11.7v0.01" })
  ] });
}
function Field({ label, description, children, className = "" }) {
  return /* @__PURE__ */ jsxs("div", { className: `obs-field ${className}`, children: [
    /* @__PURE__ */ jsx("div", { className: "obs-field__label", children: label }),
    children,
    description ? /* @__PURE__ */ jsx("p", { className: "obs-field__description", children: description }) : null
  ] });
}
function Toggle({
  checked,
  onChange,
  disabled = false,
  onLabel = "Enabled",
  offLabel = "Disabled"
}) {
  return /* @__PURE__ */ jsxs(
    "button",
    {
      type: "button",
      className: `obs-toggle${checked ? " is-on" : ""}`,
      "aria-pressed": checked,
      disabled,
      onClick: () => onChange(!checked),
      children: [
        /* @__PURE__ */ jsx("span", { className: "obs-toggle__track", children: /* @__PURE__ */ jsx("span", { className: "obs-toggle__thumb" }) }),
        /* @__PURE__ */ jsx("span", { children: checked ? onLabel : offLabel })
      ]
    }
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
  onChange
}) {
  const handleChange = (event) => {
    const raw = event.target.value;
    onChange(raw === "" ? void 0 : Number(raw));
  };
  return /* @__PURE__ */ jsxs("div", { className: "obs-number-field", children: [
    /* @__PURE__ */ jsx(
      "input",
      {
        id,
        type: "number",
        value: value ?? "",
        min,
        max,
        step,
        placeholder,
        disabled,
        onChange: handleChange
      }
    ),
    suffix ? /* @__PURE__ */ jsx("span", { className: "obs-number-field__suffix", children: suffix }) : null
  ] });
}
function pickUnitIndex(base, units) {
  if (base === void 0 || base <= 0) return units.length - 1;
  for (let index = units.length - 1; index >= 0; index -= 1) {
    if (base % units[index].factor === 0) return index;
  }
  for (let index = units.length - 1; index >= 0; index -= 1) {
    if (base >= units[index].factor) return index;
  }
  return 0;
}
function formatUnitValue(base, factor) {
  return String(Math.round(base / factor * 100) / 100);
}
function UnitNumberField({
  id,
  baseValue,
  units,
  minBase = 0,
  disabled = false,
  onChange
}) {
  const [unitIndex, setUnitIndex] = useState(() => pickUnitIndex(baseValue, units));
  const unit = units[Math.min(unitIndex, units.length - 1)];
  const [text, setText] = useState(
    () => baseValue === void 0 ? "" : formatUnitValue(baseValue, unit.factor)
  );
  const [lastBase, setLastBase] = useState(baseValue);
  if (baseValue !== lastBase) {
    setLastBase(baseValue);
    setText(baseValue === void 0 ? "" : formatUnitValue(baseValue, unit.factor));
  }
  const emit = (nextBase) => {
    setLastBase(nextBase);
    onChange(nextBase);
  };
  const handleValueChange = (event) => {
    const raw = event.target.value;
    setText(raw);
    if (raw === "") {
      emit(void 0);
      return;
    }
    const parsed = Number(raw);
    if (!Number.isFinite(parsed) || parsed < 0) return;
    emit(Math.max(minBase, Math.round(parsed * unit.factor)));
  };
  const handleBlur = () => {
    setText(baseValue === void 0 ? "" : formatUnitValue(baseValue, unit.factor));
  };
  const handleUnitChange = (event) => {
    const nextIndex = Number(event.target.value);
    const nextUnit = units[nextIndex] ?? unit;
    setUnitIndex(nextIndex);
    setText(baseValue === void 0 ? "" : formatUnitValue(baseValue, nextUnit.factor));
  };
  return /* @__PURE__ */ jsxs("div", { className: "obs-unit-field", children: [
    /* @__PURE__ */ jsx(
      "input",
      {
        id,
        type: "number",
        min: 0,
        value: text,
        disabled,
        onChange: handleValueChange,
        onBlur: handleBlur
      }
    ),
    /* @__PURE__ */ jsx(
      "select",
      {
        "aria-label": "Unit",
        value: String(Math.min(unitIndex, units.length - 1)),
        disabled,
        onChange: handleUnitChange,
        children: units.map((option, index) => /* @__PURE__ */ jsx("option", { value: String(index), children: option.label }, option.label))
      }
    )
  ] });
}
function SelectField({
  id,
  value,
  options,
  disabled = false,
  onChange
}) {
  return /* @__PURE__ */ jsxs(
    "select",
    {
      id,
      value: value ?? "",
      disabled,
      onChange: (event) => onChange(event.target.value || void 0),
      children: [
        /* @__PURE__ */ jsx("option", { value: "", children: "Use default" }),
        options.map((option) => /* @__PURE__ */ jsx("option", { value: option.value, children: option.label }, option.value))
      ]
    }
  );
}
function TextField({
  id,
  value,
  placeholder,
  disabled = false,
  templateHint = false,
  onChange
}) {
  const [focused, setFocused] = useState(false);
  const raw = value ?? "";
  const isTemplate = /\$\{[^}]*\}/.test(raw);
  const template = templateHint ? /^\$\{([A-Za-z0-9_]+):(.*)\}$/.exec(raw) : null;
  const display = template && !focused ? `\${${template[1]}}` : raw;
  const input = /* @__PURE__ */ jsx(
    "input",
    {
      id,
      type: "text",
      className: isTemplate ? "is-template" : void 0,
      value: display,
      placeholder,
      disabled,
      onFocus: () => setFocused(true),
      onBlur: () => setFocused(false),
      onChange: (event) => onChange(event.target.value || void 0)
    }
  );
  if (!templateHint) return input;
  return /* @__PURE__ */ jsxs(Fragment, { children: [
    input,
    template ? /* @__PURE__ */ jsxs("span", { className: "obs-field__default", children: [
      "default: ",
      template[2]
    ] }) : null
  ] });
}
function StatusPill({
  health,
  state
}) {
  let tone;
  let text;
  if (state === "error") {
    tone = "bad";
    text = "engine unavailable";
  } else if (!health) {
    tone = "muted";
    text = "updating\u2026";
  } else {
    const status = asString(health.status, "unknown");
    tone = statusTone(status);
    if (status === "healthy") text = "healthy pipeline \xB7 updates every 5 s";
    else if (status === "degraded") text = "degraded pipeline \xB7 updates every 5 s";
    else if (status === "error" || status === "failed") text = "pipeline failure";
    else text = `pipeline: ${titleCase(status)}`;
  }
  return /* @__PURE__ */ jsxs("span", { className: `obs-status-pill obs-status-pill--${tone}`, role: "status", children: [
    /* @__PURE__ */ jsx("span", { className: `obs-status-dot obs-status-dot--${tone}` }),
    /* @__PURE__ */ jsx("span", { children: text })
  ] });
}
function UsageMeter({ label, used, limit, caption, tone }) {
  const pct = used !== void 0 && limit !== void 0 && limit > 0 ? Math.min(100, Math.round(used / limit * 100)) : 0;
  return /* @__PURE__ */ jsxs("div", { className: "obs-meter", children: [
    /* @__PURE__ */ jsxs("div", { className: "obs-meter__top", children: [
      /* @__PURE__ */ jsx("span", { className: "obs-meter__label", children: label }),
      /* @__PURE__ */ jsxs("span", { className: "obs-meter__value", children: [
        formatBytes(used),
        " ",
        /* @__PURE__ */ jsxs("span", { children: [
          "de ",
          formatBytes(limit)
        ] })
      ] })
    ] }),
    /* @__PURE__ */ jsx("div", { className: "obs-meter__track", children: /* @__PURE__ */ jsx(
      "div",
      {
        className: `obs-meter__fill${pct >= 90 ? " obs-meter__fill--warn" : ""}`,
        style: { width: `${pct}%` }
      }
    ) }),
    /* @__PURE__ */ jsxs("div", { className: "obs-meter__caption", children: [
      /* @__PURE__ */ jsx("span", { className: `obs-status-dot obs-status-dot--${tone}` }),
      /* @__PURE__ */ jsx("span", { children: caption })
    ] })
  ] });
}
function PersistenceSection({
  props,
  health
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
  let caption = "waiting for engine data\u2026";
  let tone = "muted";
  if (hasData) {
    if (droppedSpans > 0) {
      caption = `${droppedSpans.toLocaleString("en-US")} spans dropped`;
      tone = "warn";
    } else {
      caption = "traces.db + WAL \xB7 no trace loss";
      tone = "good";
    }
  }
  return /* @__PURE__ */ jsxs("section", { className: "obs-card obs-focus-card", children: [
    /* @__PURE__ */ jsxs("div", { className: "obs-card__header", children: [
      /* @__PURE__ */ jsxs("div", { className: "obs-card__heading", children: [
        /* @__PURE__ */ jsx("span", { className: `obs-card__icon${enabled ? "" : " obs-card__icon--muted"}`, children: /* @__PURE__ */ jsx(DatabaseIcon, {}) }),
        /* @__PURE__ */ jsxs("div", { className: "obs-card__titles", children: [
          /* @__PURE__ */ jsx("h3", { children: "Disk persistence" }),
          /* @__PURE__ */ jsx("p", { className: "obs-card__description", children: "Trace history stored in a local SQLite database." })
        ] })
      ] }),
      /* @__PURE__ */ jsx(
        Toggle,
        {
          checked: enabled,
          onLabel: "Enabled",
          offLabel: "Disabled",
          onChange: (next) => updateObject(props, "trace_storage", "enabled", next)
        }
      )
    ] }),
    enabled ? /* @__PURE__ */ jsx(
      UsageMeter,
      {
        label: "Storage used",
        used: hasData ? asNumber(archive.physical_bytes, 0) : void 0,
        limit: maxDiskBytes,
        caption,
        tone
      }
    ) : /* @__PURE__ */ jsxs("div", { className: "obs-banner obs-banner--warn", children: [
      /* @__PURE__ */ jsx(WarningIcon, {}),
      /* @__PURE__ */ jsxs("div", { children: [
        /* @__PURE__ */ jsx("strong", { children: "Memory only \u2014 history is lost when the engine restarts." }),
        /* @__PURE__ */ jsxs("span", { children: [
          "Without persistence, traces exist only in the cache: once it reaches the",
          " ",
          formatBytes(memoryMaxBytes),
          " limit, the oldest traces are permanently discarded."
        ] })
      ] })
    ] }),
    /* @__PURE__ */ jsx(
      Field,
      {
        label: "Storage limit",
        description: "Maximum directory size. Once the limit is reached, the oldest traces are deleted first. Minimum: 64 MiB.",
        children: /* @__PURE__ */ jsx(
          UnitNumberField,
          {
            id: "trace-storage-max-disk",
            baseValue: maxDiskBytes,
            units: BYTE_UNITS,
            minBase: 64 * MIB,
            disabled: !enabled,
            onChange: (value) => updateObject(props, "trace_storage", "max_disk_bytes", value)
          }
        )
      }
    ),
    /* @__PURE__ */ jsx(
      Field,
      {
        label: "Retain traces for",
        description: "Traces older than this are deleted even when space is available. Use 0 to retain them until the storage limit is reached.",
        children: /* @__PURE__ */ jsx(
          UnitNumberField,
          {
            id: "trace-storage-retention",
            baseValue: asNumber(storage.retention_seconds),
            units: RETENTION_UNITS,
            disabled: !enabled,
            onChange: (value) => updateObject(props, "trace_storage", "retention_seconds", value)
          }
        )
      }
    ),
    /* @__PURE__ */ jsx(
      Field,
      {
        label: "Data directory",
        description: "Directory reserved for the database and WAL files. Relative paths start from the engine directory.",
        children: /* @__PURE__ */ jsx(
          TextField,
          {
            id: "trace-storage-directory",
            value: asString(storage.directory),
            disabled: !enabled,
            onChange: (value) => updateObject(props, "trace_storage", "directory", value)
          }
        )
      }
    )
  ] });
}
function MemorySection({
  props,
  health
}) {
  const root = asObject(props.value);
  const rawStorage = asObject(root.trace_storage);
  const storage = { ...DEFAULT_TRACE_STORAGE, ...rawStorage };
  const persistenceEnabled = asBoolean(storage.enabled, true);
  const memoryMaxBytes = asNumber(storage.memory_max_bytes);
  const watermarkRatio = asNumber(storage.memory_low_watermark_ratio, 0.75);
  const spanDetails = detailsOf(health?.components?.spans);
  const hasData = health !== null;
  const hotBytes = hasData ? asNumber(spanDetails.hot_bytes, 0) : void 0;
  const storedSpans = asNumber(spanDetails.stored_spans, 0);
  const pct = hotBytes !== void 0 && memoryMaxBytes > 0 ? hotBytes / memoryMaxBytes * 100 : 0;
  let caption = "waiting for engine data\u2026";
  let tone = "muted";
  if (hasData) {
    const spansLabel = `${storedSpans.toLocaleString("en-US")} spans in memory`;
    if (pct >= 90) {
      caption = persistenceEnabled ? `${spansLabel} \xB7 near the limit` : `${spansLabel} \xB7 near the limit, older traces will be discarded`;
      tone = "warn";
    } else {
      caption = spansLabel;
      tone = "good";
    }
  }
  return /* @__PURE__ */ jsxs("section", { className: "obs-card obs-focus-card", children: [
    /* @__PURE__ */ jsx("div", { className: "obs-card__header", children: /* @__PURE__ */ jsxs("div", { className: "obs-card__heading", children: [
      /* @__PURE__ */ jsx("span", { className: "obs-card__icon", children: /* @__PURE__ */ jsx(ChipIcon, {}) }),
      /* @__PURE__ */ jsxs("div", { className: "obs-card__titles", children: [
        /* @__PURE__ */ jsx("h3", { children: "Memory usage" }),
        /* @__PURE__ */ jsx("p", { className: "obs-card__description", children: persistenceEnabled ? "Recent traces stay in RAM for immediate queries." : "With persistence disabled, the cache is the only storage." })
      ] })
    ] }) }),
    /* @__PURE__ */ jsx(
      UsageMeter,
      {
        label: "Cache used",
        used: hotBytes,
        limit: memoryMaxBytes,
        caption,
        tone
      }
    ),
    /* @__PURE__ */ jsx(
      Field,
      {
        label: "Cache limit",
        description: persistenceEnabled ? "Above this limit, older traces leave memory and are read from disk instead. Minimum: 16 MiB." : "Above this limit, older traces leave memory. Without persistence, they are lost. Minimum: 16 MiB.",
        children: /* @__PURE__ */ jsx(
          NumberField,
          {
            id: "trace-storage-memory",
            value: Math.round(memoryMaxBytes / MIB),
            min: 16,
            step: 16,
            suffix: "MiB",
            onChange: (value) => updateObject(
              props,
              "trace_storage",
              "memory_max_bytes",
              value === void 0 ? void 0 : Math.max(16, value) * MIB
            )
          }
        )
      }
    ),
    /* @__PURE__ */ jsx(
      Field,
      {
        label: "Keep after cleanup",
        description: "Each cleanup frees memory until usage falls to this fraction of the limit. Between 50% and 95%.",
        children: /* @__PURE__ */ jsx(
          NumberField,
          {
            id: "trace-storage-low-watermark",
            value: Math.round(watermarkRatio * 100),
            min: 50,
            max: 95,
            step: 5,
            suffix: "% do limite",
            onChange: (value) => updateObject(
              props,
              "trace_storage",
              "memory_low_watermark_ratio",
              value === void 0 ? void 0 : Math.min(95, Math.max(50, value)) / 100
            )
          }
        )
      }
    ),
    /* @__PURE__ */ jsx(
      Field,
      {
        label: "Discard incomplete trace after",
        description: "A trace that never receives its final span is closed and released from memory after this time.",
        children: /* @__PURE__ */ jsx(
          UnitNumberField,
          {
            id: "trace-storage-pending-age",
            baseValue: asNumber(storage.pending_max_age_seconds),
            units: PENDING_UNITS,
            onChange: (value) => updateObject(props, "trace_storage", "pending_max_age_seconds", value)
          }
        )
      }
    ),
    /* @__PURE__ */ jsx(
      Field,
      {
        label: "Spans retained by exporter",
        description: "Applies to the Memory and Memory + OTLP exporters. Empty uses the engine default.",
        className: "obs-field--separated",
        children: /* @__PURE__ */ jsx(
          NumberField,
          {
            id: "pipeline-memory-spans",
            value: root.memory_max_spans === void 0 ? void 0 : asNumber(root.memory_max_spans),
            min: 1,
            step: 1e3,
            suffix: "spans",
            placeholder: "engine default",
            onChange: (value) => updateRoot(props, "memory_max_spans", value)
          }
        )
      }
    )
  ] });
}
function PipelineSection({ props }) {
  const root = asObject(props.value);
  const enabled = asBoolean(root.enabled, true);
  const exporter = asString(root.exporter);
  return /* @__PURE__ */ jsxs("section", { className: "obs-card", children: [
    /* @__PURE__ */ jsxs("div", { className: "obs-card__header", children: [
      /* @__PURE__ */ jsxs("div", { className: "obs-card__titles", children: [
        /* @__PURE__ */ jsx("h3", { children: "Collection and export" }),
        /* @__PURE__ */ jsx("p", { className: "obs-card__description", children: "How the engine collects, samples, and forwards telemetry." })
      ] }),
      /* @__PURE__ */ jsx(
        Toggle,
        {
          checked: enabled,
          onLabel: "Enabled",
          offLabel: "Disabled",
          onChange: (next) => updateRoot(props, "enabled", next)
        }
      )
    ] }),
    /* @__PURE__ */ jsxs("div", { className: "obs-form-grid obs-form-grid--three", children: [
      /* @__PURE__ */ jsx(Field, { label: "Exporter", description: "Where completed spans are sent.", children: /* @__PURE__ */ jsx(
        SelectField,
        {
          id: "pipeline-exporter",
          value: exporter || void 0,
          options: [
            { value: "memory", label: "Memory" },
            { value: "otlp", label: "OTLP" },
            { value: "both", label: "Memory + OTLP" }
          ],
          onChange: (value) => updateRoot(props, "exporter", value)
        }
      ) }),
      /* @__PURE__ */ jsx(Field, { label: "Service name", description: "Identifies this engine in exported traces.", children: /* @__PURE__ */ jsx(
        TextField,
        {
          id: "pipeline-service-name",
          value: asString(root.service_name),
          placeholder: "iii",
          onChange: (value) => updateRoot(props, "service_name", value)
        }
      ) }),
      /* @__PURE__ */ jsx(Field, { label: "Sampling", description: "1.0 keeps all traces; 0.1 keeps 1 in 10.", children: /* @__PURE__ */ jsx(
        NumberField,
        {
          id: "pipeline-sampling-ratio",
          value: root.sampling_ratio === void 0 ? void 0 : asNumber(root.sampling_ratio),
          min: 0,
          max: 1,
          step: 0.05,
          suffix: "ratio",
          onChange: (value) => updateRoot(props, "sampling_ratio", value)
        }
      ) }),
      /* @__PURE__ */ jsx(Field, { label: "OTLP endpoint", description: "Used only with the OTLP or Memory + OTLP exporter.", children: /* @__PURE__ */ jsx(
        TextField,
        {
          id: "pipeline-endpoint",
          value: asString(root.endpoint),
          placeholder: "http://localhost:4317",
          onChange: (value) => updateRoot(props, "endpoint", value)
        }
      ) }),
      /* @__PURE__ */ jsx(Field, { label: "Live spans", description: "Includes in-progress traces in Console queries.", children: /* @__PURE__ */ jsx(
        Toggle,
        {
          checked: asBoolean(root.live_spans),
          onLabel: "Enabled",
          offLabel: "Disabled",
          onChange: (next) => updateRoot(props, "live_spans", next)
        }
      ) })
    ] })
  ] });
}
function SamplingSection({ props }) {
  const sampling = asObject(asObject(props.value).sampling);
  const rules = Array.isArray(sampling.rules) ? sampling.rules : [];
  const rateLimit = asObject(sampling.rate_limit);
  return /* @__PURE__ */ jsxs("section", { className: "obs-card", children: [
    /* @__PURE__ */ jsx("div", { className: "obs-card__header", children: /* @__PURE__ */ jsxs("div", { children: [
      /* @__PURE__ */ jsx("p", { className: "obs-kicker", children: "Advanced \xB7 Sampling" }),
      /* @__PURE__ */ jsx("h3", { children: "Sampling rules" }),
      /* @__PURE__ */ jsx("p", { className: "obs-card__description", children: "Prioritize important operations without losing control of volume." })
    ] }) }),
    /* @__PURE__ */ jsxs("div", { className: "obs-form-grid obs-form-grid--three", children: [
      /* @__PURE__ */ jsx(Field, { label: "Default ratio", children: /* @__PURE__ */ jsx(
        NumberField,
        {
          id: "sampling-default",
          value: sampling.default === void 0 ? void 0 : asNumber(sampling.default),
          min: 0,
          max: 1,
          step: 0.05,
          suffix: "ratio",
          onChange: (value) => updateObject(props, "sampling", "default", value)
        }
      ) }),
      /* @__PURE__ */ jsx(Field, { label: "Parent-based", description: "Inherit the decision from the parent span.", children: /* @__PURE__ */ jsx(
        Toggle,
        {
          checked: asBoolean(sampling.parent_based),
          onChange: (next) => updateObject(props, "sampling", "parent_based", next)
        }
      ) }),
      /* @__PURE__ */ jsx(Field, { label: "Rate limit", children: /* @__PURE__ */ jsx(
        NumberField,
        {
          id: "sampling-rate-limit",
          value: rateLimit.max_traces_per_second === void 0 ? void 0 : asNumber(rateLimit.max_traces_per_second),
          min: 1,
          step: 10,
          suffix: "traces/s",
          onChange: (value) => updateNestedObject(props, "sampling", "rate_limit", "max_traces_per_second", value)
        }
      ) })
    ] }),
    /* @__PURE__ */ jsxs("div", { className: "obs-subsection", children: [
      /* @__PURE__ */ jsxs("div", { className: "obs-subsection__header", children: [
        /* @__PURE__ */ jsxs("div", { children: [
          /* @__PURE__ */ jsx("strong", { children: "Rules by operation" }),
          /* @__PURE__ */ jsx("span", { children: "Applied in the order shown." })
        ] }),
        /* @__PURE__ */ jsx(
          "button",
          {
            type: "button",
            className: "obs-button obs-button--secondary",
            onClick: () => addNestedArrayItem(props, "sampling", "rules", { rate: 0.1 }),
            children: "Add rule"
          }
        )
      ] }),
      rules.length === 0 ? /* @__PURE__ */ jsx("div", { className: "obs-empty", children: "No specific rules. The default ratio will be used." }) : /* @__PURE__ */ jsx("div", { className: "obs-rules", children: rules.map((rawRule, index) => {
        const rule = asObject(rawRule);
        return /* @__PURE__ */ jsxs("div", { className: "obs-rule", children: [
          /* @__PURE__ */ jsx("span", { className: "obs-rule__index", children: index + 1 }),
          /* @__PURE__ */ jsx(
            TextField,
            {
              id: `sampling-operation-${index}`,
              value: asString(rule.operation),
              placeholder: "operation, ex.: checkout.*",
              onChange: (value) => updateNestedArrayItem(props, "sampling", "rules", index, (item) => {
                if (value === void 0) delete item.operation;
                else item.operation = value;
                return item;
              })
            }
          ),
          /* @__PURE__ */ jsx(
            TextField,
            {
              id: `sampling-service-${index}`,
              value: asString(rule.service),
              placeholder: "service (opcional)",
              onChange: (value) => updateNestedArrayItem(props, "sampling", "rules", index, (item) => {
                if (value === void 0) delete item.service;
                else item.service = value;
                return item;
              })
            }
          ),
          /* @__PURE__ */ jsx(
            NumberField,
            {
              id: `sampling-rate-${index}`,
              value: asNumber(rule.rate, 0.1),
              min: 0,
              max: 1,
              step: 0.05,
              suffix: "ratio",
              onChange: (value) => updateNestedArrayItem(props, "sampling", "rules", index, (item) => ({
                ...item,
                rate: value ?? 0
              }))
            }
          ),
          /* @__PURE__ */ jsx(
            "button",
            {
              type: "button",
              className: "obs-icon-button",
              "aria-label": `Remover regra ${index + 1}`,
              onClick: () => removeNestedArrayItem(props, "sampling", "rules", index),
              children: "\xD7"
            }
          )
        ] }, `sampling-rule-${index}`);
      }) })
    ] })
  ] });
}
function MetricsLogsSection({ props }) {
  const root = asObject(props.value);
  return /* @__PURE__ */ jsxs("section", { className: "obs-card", children: [
    /* @__PURE__ */ jsx("div", { className: "obs-card__header", children: /* @__PURE__ */ jsxs("div", { children: [
      /* @__PURE__ */ jsx("p", { className: "obs-kicker", children: "Advanced \xB7 Signals" }),
      /* @__PURE__ */ jsx("h3", { children: "Metrics and logs" }),
      /* @__PURE__ */ jsx("p", { className: "obs-card__description", children: "Adjust retention and export without changing traces." })
    ] }) }),
    /* @__PURE__ */ jsxs("div", { className: "obs-signal-grid", children: [
      /* @__PURE__ */ jsxs("div", { className: "obs-signal-panel", children: [
        /* @__PURE__ */ jsxs("div", { className: "obs-signal-panel__header", children: [
          /* @__PURE__ */ jsxs("div", { children: [
            /* @__PURE__ */ jsx("strong", { children: "Metrics" }),
            /* @__PURE__ */ jsx("span", { children: "Aggregated engine metrics." })
          ] }),
          /* @__PURE__ */ jsx(
            Toggle,
            {
              checked: asBoolean(root.metrics_enabled),
              onChange: (next) => updateRoot(props, "metrics_enabled", next)
            }
          )
        ] }),
        /* @__PURE__ */ jsx(Field, { label: "Exporter", children: /* @__PURE__ */ jsx(
          SelectField,
          {
            id: "metrics-exporter",
            value: asString(root.metrics_exporter) || void 0,
            options: [{ value: "memory", label: "Memory" }, { value: "otlp", label: "OTLP" }],
            onChange: (value) => updateRoot(props, "metrics_exporter", value)
          }
        ) }),
        /* @__PURE__ */ jsx(Field, { label: "Max count", children: /* @__PURE__ */ jsx(
          NumberField,
          {
            id: "metrics-max-count",
            value: root.metrics_max_count === void 0 ? void 0 : asNumber(root.metrics_max_count),
            min: 1,
            step: 1e3,
            suffix: "items",
            onChange: (value) => updateRoot(props, "metrics_max_count", value)
          }
        ) }),
        /* @__PURE__ */ jsx(Field, { label: "Retention", children: /* @__PURE__ */ jsx(
          NumberField,
          {
            id: "metrics-retention",
            value: root.metrics_retention_seconds === void 0 ? void 0 : asNumber(root.metrics_retention_seconds),
            min: 1,
            step: 60,
            suffix: "seconds",
            onChange: (value) => updateRoot(props, "metrics_retention_seconds", value)
          }
        ) })
      ] }),
      /* @__PURE__ */ jsxs("div", { className: "obs-signal-panel", children: [
        /* @__PURE__ */ jsxs("div", { className: "obs-signal-panel__header", children: [
          /* @__PURE__ */ jsxs("div", { children: [
            /* @__PURE__ */ jsx("strong", { children: "Logs" }),
            /* @__PURE__ */ jsx("span", { children: "Structured events and engine output." })
          ] }),
          /* @__PURE__ */ jsx(Toggle, { checked: asBoolean(root.logs_enabled, true), onChange: (next) => updateRoot(props, "logs_enabled", next) })
        ] }),
        /* @__PURE__ */ jsx(Field, { label: "Exporter", children: /* @__PURE__ */ jsx(
          SelectField,
          {
            id: "logs-exporter",
            value: asString(root.logs_exporter) || void 0,
            options: [{ value: "memory", label: "Memory" }, { value: "otlp", label: "OTLP" }, { value: "both", label: "Memory + OTLP" }],
            onChange: (value) => updateRoot(props, "logs_exporter", value)
          }
        ) }),
        /* @__PURE__ */ jsx(Field, { label: "Sampling", children: /* @__PURE__ */ jsx(
          NumberField,
          {
            id: "logs-sampling-ratio",
            value: asNumber(root.logs_sampling_ratio, 1),
            min: 0,
            max: 1,
            step: 0.05,
            suffix: "ratio",
            onChange: (value) => updateRoot(props, "logs_sampling_ratio", value)
          }
        ) }),
        /* @__PURE__ */ jsx(Field, { label: "Console output", children: /* @__PURE__ */ jsx(Toggle, { checked: asBoolean(root.logs_console_output, true), onChange: (next) => updateRoot(props, "logs_console_output", next) }) })
      ] })
    ] })
  ] });
}
function TextJsonField({
  id,
  label,
  value,
  onChange
}) {
  const [text, setText] = useState(value === void 0 ? "" : JSON.stringify(value, null, 2));
  const [invalid, setInvalid] = useState(false);
  useEffect(() => {
    setText(value === void 0 ? "" : JSON.stringify(value, null, 2));
  }, [value]);
  const handleBlur = () => {
    if (text.trim() === "") {
      setInvalid(false);
      onChange(void 0);
      return;
    }
    try {
      const parsed = JSON.parse(text);
      setInvalid(false);
      onChange(parsed);
    } catch {
      setInvalid(true);
    }
  };
  return /* @__PURE__ */ jsxs(Field, { label, description: "Advanced JSON; unedited fields remain preserved.", children: [
    /* @__PURE__ */ jsx(
      "textarea",
      {
        id,
        className: invalid ? "is-invalid" : "",
        value: text,
        rows: 7,
        spellCheck: false,
        onChange: (event) => setText(event.target.value),
        onBlur: handleBlur
      }
    ),
    invalid ? /* @__PURE__ */ jsx("span", { className: "obs-field__error", children: "Invalid JSON. Fix it before leaving this field." }) : null
  ] });
}
function AdvancedSections({ props }) {
  const root = asObject(props.value);
  return /* @__PURE__ */ jsxs("div", { className: "obs-stack", children: [
    /* @__PURE__ */ jsx(SamplingSection, { props }),
    /* @__PURE__ */ jsx(MetricsLogsSection, { props }),
    /* @__PURE__ */ jsxs("section", { className: "obs-card", children: [
      /* @__PURE__ */ jsx("div", { className: "obs-card__header", children: /* @__PURE__ */ jsxs("div", { children: [
        /* @__PURE__ */ jsx("p", { className: "obs-kicker", children: "Advanced \xB7 Runtime" }),
        /* @__PURE__ */ jsx("h3", { children: "Runtime and rules" }),
        /* @__PURE__ */ jsx("p", { className: "obs-card__description", children: "Less frequent options are separated to keep the overview operational." })
      ] }) }),
      /* @__PURE__ */ jsxs("div", { className: "obs-form-grid obs-form-grid--three", children: [
        /* @__PURE__ */ jsx(Field, { label: "Service version", children: /* @__PURE__ */ jsx(
          TextField,
          {
            id: "runtime-service-version",
            value: asString(root.service_version),
            templateHint: true,
            onChange: (value) => updateRoot(props, "service_version", value)
          }
        ) }),
        /* @__PURE__ */ jsx(Field, { label: "Service namespace", children: /* @__PURE__ */ jsx(TextField, { id: "runtime-service-namespace", value: asString(root.service_namespace), onChange: (value) => updateRoot(props, "service_namespace", value) }) }),
        /* @__PURE__ */ jsx(Field, { label: "Log level", children: /* @__PURE__ */ jsx(
          SelectField,
          {
            id: "runtime-level",
            value: asString(root.level) || void 0,
            options: ["trace", "debug", "info", "warn", "error"].map((value) => ({ value, label: titleCase(value) })),
            onChange: (value) => updateRoot(props, "level", value)
          }
        ) }),
        /* @__PURE__ */ jsx(Field, { label: "Log format", children: /* @__PURE__ */ jsx(
          SelectField,
          {
            id: "runtime-format",
            value: asString(root.format) || void 0,
            options: [{ value: "default", label: "Human readable" }, { value: "json", label: "JSON" }],
            onChange: (value) => updateRoot(props, "format", value)
          }
        ) }),
        /* @__PURE__ */ jsx(Field, { label: "Logs max count", children: /* @__PURE__ */ jsx(
          NumberField,
          {
            id: "logs-max-count",
            value: root.logs_max_count === void 0 ? void 0 : asNumber(root.logs_max_count),
            min: 1,
            step: 100,
            suffix: "items",
            onChange: (value) => updateRoot(props, "logs_max_count", value)
          }
        ) }),
        /* @__PURE__ */ jsx(Field, { label: "Logs retention", children: /* @__PURE__ */ jsx(
          NumberField,
          {
            id: "logs-retention",
            value: root.logs_retention_seconds === void 0 ? void 0 : asNumber(root.logs_retention_seconds),
            min: 1,
            step: 60,
            suffix: "seconds",
            onChange: (value) => updateRoot(props, "logs_retention_seconds", value)
          }
        ) })
      ] }),
      /* @__PURE__ */ jsxs("div", { className: "obs-advanced-json", children: [
        /* @__PURE__ */ jsx(TextJsonField, { id: "alerts-json", label: "Alert rules", value: root.alerts, onChange: (value) => updateRoot(props, "alerts", value) }),
        /* @__PURE__ */ jsx(TextJsonField, { id: "collapse-spans-json", label: "Span collapse rules", value: root.collapse_spans, onChange: (value) => updateRoot(props, "collapse_spans", value) })
      ] })
    ] })
  ] });
}
function ErrorSummary({ errors }) {
  const entries = useMemo(() => errors ? Array.from(errors.entries()) : [], [errors]);
  if (entries.length === 0) return null;
  return /* @__PURE__ */ jsxs("div", { className: "obs-error-summary", role: "alert", children: [
    /* @__PURE__ */ jsx("strong", { children: "Review the fields before saving" }),
    entries.map(([path, message]) => /* @__PURE__ */ jsxs("span", { children: [
      path || "configuration",
      ": ",
      message
    ] }, `${path}:${message}`))
  ] });
}
function ObservabilityConfigPage({ healthCheck, ...props }) {
  const [section, setSection] = useState("overview");
  const [health, setHealth] = useState(null);
  const [healthState, setHealthState] = useState("loading");
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
    const timer = window.setInterval(() => void refresh(), 5e3);
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
  return /* @__PURE__ */ jsxs("div", { className: "iii-observability-ui", children: [
    /* @__PURE__ */ jsxs("div", { className: "obs-toolbar", children: [
      /* @__PURE__ */ jsx("div", { className: "obs-tabs", role: "tablist", "aria-label": "Configuration sections", children: ["overview", "advanced"].map((id) => /* @__PURE__ */ jsx(
        "button",
        {
          type: "button",
          role: "tab",
          "aria-selected": section === id,
          className: section === id ? "is-active" : "",
          onClick: () => setSection(id),
          children: id === "overview" ? "Overview" : "Advanced"
        },
        id
      )) }),
      /* @__PURE__ */ jsx(StatusPill, { health, state: healthState })
    ] }),
    /* @__PURE__ */ jsx("main", { className: "obs-content", role: "tabpanel", children: section === "overview" ? /* @__PURE__ */ jsxs("div", { className: "obs-stack", children: [
      /* @__PURE__ */ jsxs("div", { className: "obs-focus-grid", children: [
        /* @__PURE__ */ jsx(PersistenceSection, { props, health }),
        /* @__PURE__ */ jsx(MemorySection, { props, health })
      ] }),
      /* @__PURE__ */ jsx(PipelineSection, { props })
    ] }) : /* @__PURE__ */ jsx(AdvancedSections, { props }) }),
    /* @__PURE__ */ jsx(ErrorSummary, { errors: props.errors })
  ] });
}
function setup(host) {
  const healthCheck = () => host.iii.trigger("engine::health::check", {});
  const RegisteredForm = (props) => /* @__PURE__ */ jsx(
    ObservabilityConfigPage,
    {
      ...props,
      healthCheck
    }
  );
  return host.configForms.register("iii-observability", RegisteredForm, { layout: "full" });
}
export {
  ObservabilityConfigPage,
  setup as default
};
