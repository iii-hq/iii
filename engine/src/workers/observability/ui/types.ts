import type { ComponentType } from "react";

export type JsonPrimitive = string | number | boolean | null;
export type JsonValue = JsonPrimitive | JsonValue[] | { [key: string]: JsonValue };
export type JsonObject = { [key: string]: JsonValue };
export type JsonSchema = JsonObject;

export interface ConfigFormProps {
  id: string;
  schema: JsonSchema | null;
  value: JsonValue;
  onChange(next: JsonValue): void;
  errors?: ReadonlyMap<string, string>;
  focusField?: readonly string[];
}

export interface ConfigFormHost {
  iii: {
    trigger<T = unknown>(
      functionId: string,
      payload?: Record<string, JsonValue>,
    ): Promise<T>;
  };
  configForms: {
    register(
      configurationId: string,
      component: ComponentType<ConfigFormProps>,
      options?: { layout?: "contained" | "full" },
    ): () => void;
  };
}

export interface HealthComponent {
  status?: string;
  details?: JsonValue;
}

export interface HealthCheckResult {
  status?: string;
  version?: string;
  timestamp?: number;
  components?: {
    otel?: HealthComponent;
    metrics?: HealthComponent;
    logs?: HealthComponent;
    spans?: HealthComponent;
  };
}
