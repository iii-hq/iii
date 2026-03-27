export type ToolDef = {
  name: string;
  function_id: string;
  description: string;
  input_schema: Record<string, unknown>;
};

export const TOOLS: ToolDef[] = [
  {
    name: "browser_navigate",
    function_id: "proof::browser::navigate",
    description: "Navigate to a URL. Returns the page accessibility snapshot after navigation.",
    input_schema: {
      type: "object",
      properties: { url: { type: "string", description: "URL to navigate to" } },
      required: ["url"],
    },
  },
  {
    name: "browser_snapshot",
    function_id: "proof::browser::snapshot",
    description: "Get the current page accessibility tree. Interactive elements have [ref=eN] markers. Use these refs in click, type, select, and press tools.",
    input_schema: { type: "object", properties: {} },
  },
  {
    name: "browser_click",
    function_id: "proof::browser::click",
    description: "Click an element by ref ID from the snapshot. Returns updated snapshot.",
    input_schema: {
      type: "object",
      properties: { ref: { type: "string", description: "Ref ID from snapshot (e.g. 'e3')" } },
      required: ["ref"],
    },
  },
  {
    name: "browser_type",
    function_id: "proof::browser::type",
    description: "Type text into an input by ref ID. Clears existing text first. Returns updated snapshot.",
    input_schema: {
      type: "object",
      properties: {
        ref: { type: "string", description: "Ref ID from snapshot" },
        text: { type: "string", description: "Text to type" },
      },
      required: ["ref", "text"],
    },
  },
  {
    name: "browser_select",
    function_id: "proof::browser::select",
    description: "Select an option in a dropdown by ref ID. Returns updated snapshot.",
    input_schema: {
      type: "object",
      properties: {
        ref: { type: "string", description: "Ref ID from snapshot" },
        value: { type: "string", description: "Option value to select" },
      },
      required: ["ref", "value"],
    },
  },
  {
    name: "browser_press",
    function_id: "proof::browser::press",
    description: "Press a keyboard key on an element. Returns updated snapshot.",
    input_schema: {
      type: "object",
      properties: {
        ref: { type: "string", description: "Ref ID from snapshot" },
        key: { type: "string", description: "Key to press (Enter, Tab, Escape, etc.)" },
      },
      required: ["ref", "key"],
    },
  },
  {
    name: "browser_screenshot",
    function_id: "proof::browser::screenshot",
    description: "Take a screenshot of the current page. Returns base64 PNG image.",
    input_schema: {
      type: "object",
      properties: {
        description: { type: "string", description: "What you expect to see" },
      },
    },
  },
  {
    name: "browser_assert",
    function_id: "proof::browser::assert",
    description: "Record an assertion about the current page state.",
    input_schema: {
      type: "object",
      properties: {
        assertion: { type: "string", description: "What you are asserting" },
        passed: { type: "boolean", description: "Whether the assertion passed" },
      },
      required: ["assertion", "passed"],
    },
  },
  {
    name: "browser_console_logs",
    function_id: "proof::browser::console_logs",
    description: "Get console log messages from the page. Optionally filter by type and clear after reading.",
    input_schema: {
      type: "object",
      properties: {
        type: { type: "string", description: "Filter by type: log, error, warning, info" },
        clear: { type: "boolean", description: "Clear logs after reading" },
      },
    },
  },
  {
    name: "browser_network",
    function_id: "proof::browser::network",
    description: "Get network requests made by the page. Filter by method, URL substring, or resource type.",
    input_schema: {
      type: "object",
      properties: {
        method: { type: "string", description: "Filter by HTTP method (GET, POST, etc.)" },
        url_contains: { type: "string", description: "Filter by URL substring" },
        resource_type: { type: "string", description: "Filter by type: xhr, fetch, document, script, stylesheet, image" },
        clear: { type: "boolean", description: "Clear request log after reading" },
      },
    },
  },
  {
    name: "browser_performance",
    function_id: "proof::browser::performance",
    description: "Get performance metrics: FCP, DOM content loaded, TTFB, CLS, transfer size.",
    input_schema: { type: "object", properties: {} },
  },
  {
    name: "browser_exec",
    function_id: "proof::browser::exec",
    description: "Execute raw Playwright code. Has access to page, context, browser, and ref() function. Returns the result as JSON.",
    input_schema: {
      type: "object",
      properties: {
        code: { type: "string", description: "Playwright code to execute. Use ref('e3') to get locators from snapshot refs. Must return a value." },
      },
      required: ["code"],
    },
  },
];

const nameToFnId = new Map(TOOLS.map((t) => [t.name, t.function_id]));

export function toolNameToFunctionId(name: string): string {
  const fnId = nameToFnId.get(name);
  if (!fnId) throw new Error(`Unknown tool: ${name}`);
  return fnId;
}

export function getAnthropicTools() {
  return TOOLS.map((t) => ({
    name: t.name,
    description: t.description,
    input_schema: t.input_schema,
  }));
}
