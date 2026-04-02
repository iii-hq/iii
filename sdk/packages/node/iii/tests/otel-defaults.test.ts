/**
 * Unit tests for OTel default-enabled behavior.
 *
 * These tests verify that OpenTelemetry is enabled by default
 * and can be disabled via config or environment variable.
 */

import { describe, it, expect, afterEach, vi, beforeEach } from 'vitest'

// Mock WebSocket to prevent real connections
vi.mock('ws', () => {
  const MockWebSocket = vi.fn().mockImplementation(() => ({
    on: vi.fn(),
    close: vi.fn(),
    send: vi.fn(),
    readyState: 0,
  }))
  return { WebSocket: MockWebSocket, default: { WebSocket: MockWebSocket } }
})

// Mock NodeTracerProvider to avoid real tracer setup
vi.mock('@opentelemetry/sdk-trace-node', () => ({
  NodeTracerProvider: vi.fn().mockImplementation(() => ({
    register: vi.fn(),
    shutdown: vi.fn().mockResolvedValue(undefined),
  })),
}))

// Mock MeterProvider to avoid real metrics setup
vi.mock('@opentelemetry/sdk-metrics', () => ({
  MeterProvider: vi.fn().mockImplementation(() => ({
    getMeter: vi.fn().mockReturnValue({ createCounter: vi.fn() }),
    shutdown: vi.fn().mockResolvedValue(undefined),
  })),
  PeriodicExportingMetricReader: vi.fn().mockImplementation(() => ({})),
}))

// Mock LoggerProvider to avoid real log setup
vi.mock('@opentelemetry/sdk-logs', () => ({
  LoggerProvider: vi.fn().mockImplementation(() => ({
    getLogger: vi.fn().mockReturnValue({ emit: vi.fn() }),
    addLogRecordProcessor: vi.fn(),
    shutdown: vi.fn().mockResolvedValue(undefined),
  })),
  SimpleLogRecordProcessor: vi.fn().mockImplementation(() => ({})),
}))

// Mock span processor
vi.mock('@opentelemetry/sdk-trace-base', () => ({
  SimpleSpanProcessor: vi.fn().mockImplementation(() => ({})),
}))

// Mock exporters to avoid real exporter instantiation
vi.mock('../src/telemetry-system/exporters', () => ({
  EngineSpanExporter: vi.fn().mockImplementation(() => ({})),
  EngineMetricsExporter: vi.fn().mockImplementation(() => ({})),
  EngineLogExporter: vi.fn().mockImplementation(() => ({})),
}))

// Mock the shared connection to avoid real WebSocket connections
vi.mock('../src/telemetry-system/connection', () => ({
  SharedEngineConnection: vi.fn().mockImplementation(() => ({
    send: vi.fn(),
    onConnected: vi.fn(),
    getState: vi.fn().mockReturnValue('disconnected'),
    shutdown: vi.fn().mockResolvedValue(undefined),
  })),
}))

import {
  initOtel,
  shutdownOtel,
  getTracer,
  getMeter,
  getLogger,
} from '../src/telemetry-system/index'

describe('OTel default-enabled behavior', () => {
  const originalEnv = process.env

  beforeEach(() => {
    // Reset environment variables before each test
    process.env = { ...originalEnv }
    delete process.env.OTEL_ENABLED
    delete process.env.OTEL_METRICS_ENABLED
  })

  afterEach(async () => {
    // Shut down OTel to reset module-level singleton state between tests
    await shutdownOtel()
    process.env = originalEnv
  })

  it('should enable OTel by default when no config or env var is set', () => {
    // Call initOtel with no arguments — should default to enabled
    initOtel()

    // Tracer and logger should be initialized (not null)
    expect(getTracer()).not.toBeNull()
    expect(getLogger()).not.toBeNull()
  })

  it('should disable OTel when enabled: false is set in config', () => {
    initOtel({ enabled: false })

    // Tracer and logger should be null when explicitly disabled
    expect(getTracer()).toBeNull()
    expect(getLogger()).toBeNull()
  })

  it('should disable OTel when OTEL_ENABLED=false env var is set', () => {
    process.env.OTEL_ENABLED = 'false'

    initOtel()

    // Tracer and logger should be null when env var disables OTel
    expect(getTracer()).toBeNull()
    expect(getLogger()).toBeNull()
  })

  it('should disable OTel when OTEL_ENABLED=0 env var is set', () => {
    process.env.OTEL_ENABLED = '0'

    initOtel()

    // Tracer and logger should be null when env var disables OTel
    expect(getTracer()).toBeNull()
    expect(getLogger()).toBeNull()
  })

  it('should enable OTel when OTEL_ENABLED=true is set', () => {
    process.env.OTEL_ENABLED = 'true'

    initOtel()

    expect(getTracer()).not.toBeNull()
    expect(getLogger()).not.toBeNull()
  })

  it('should enable OTel when OTEL_ENABLED=1 is set', () => {
    process.env.OTEL_ENABLED = '1'

    initOtel()

    expect(getTracer()).not.toBeNull()
    expect(getLogger()).not.toBeNull()
  })

  it('should enable OTel for any non-false/non-0 OTEL_ENABLED value', () => {
    process.env.OTEL_ENABLED = 'yes'

    initOtel()

    // Any value other than 'false' or '0' is treated as enabled
    expect(getTracer()).not.toBeNull()
    expect(getLogger()).not.toBeNull()
  })

  it('should enable metrics by default when no config or env var is set', () => {
    initOtel()

    // Tracer, logger, and meter should all be initialized by default
    expect(getTracer()).not.toBeNull()
    expect(getLogger()).not.toBeNull()
    expect(getMeter()).not.toBeNull()
  })

  it('should disable metrics when metricsEnabled: false is set in config', () => {
    initOtel({ metricsEnabled: false })

    // Tracer and logger should be initialized, but meter should be null
    expect(getTracer()).not.toBeNull()
    expect(getLogger()).not.toBeNull()
    expect(getMeter()).toBeNull()
  })

  it('should disable metrics when OTEL_METRICS_ENABLED=false env var is set', () => {
    process.env.OTEL_METRICS_ENABLED = 'false'

    initOtel()

    expect(getTracer()).not.toBeNull()
    expect(getLogger()).not.toBeNull()
    expect(getMeter()).toBeNull()
  })

  it('should disable metrics when OTEL_METRICS_ENABLED=0 env var is set', () => {
    process.env.OTEL_METRICS_ENABLED = '0'

    initOtel()

    expect(getTracer()).not.toBeNull()
    expect(getLogger()).not.toBeNull()
    expect(getMeter()).toBeNull()
  })

  it('should disable OTel when OTEL_ENABLED=no env var is set', () => {
    process.env.OTEL_ENABLED = 'no'

    initOtel()

    expect(getTracer()).toBeNull()
    expect(getLogger()).toBeNull()
  })

  it('should disable OTel when OTEL_ENABLED=off env var is set', () => {
    process.env.OTEL_ENABLED = 'off'

    initOtel()

    expect(getTracer()).toBeNull()
    expect(getLogger()).toBeNull()
  })

  it('should disable OTel when OTEL_ENABLED=OFF (case-insensitive) env var is set', () => {
    process.env.OTEL_ENABLED = 'OFF'

    initOtel()

    expect(getTracer()).toBeNull()
    expect(getLogger()).toBeNull()
  })

  it('should let config.enabled override OTEL_ENABLED env var (config wins)', () => {
    process.env.OTEL_ENABLED = 'false'

    initOtel({ enabled: true })

    // Config takes precedence over env var
    expect(getTracer()).not.toBeNull()
    expect(getLogger()).not.toBeNull()
  })

  it('should let config.enabled=false override even when OTEL_ENABLED is not set', () => {
    // No env var set, but config explicitly disables
    initOtel({ enabled: false })

    expect(getTracer()).toBeNull()
    expect(getLogger()).toBeNull()
  })

  it('should let config.metricsEnabled override OTEL_METRICS_ENABLED env var', () => {
    process.env.OTEL_METRICS_ENABLED = 'false'

    initOtel({ metricsEnabled: true })

    // Config takes precedence over env var
    expect(getMeter()).not.toBeNull()
  })

  it('should disable metrics when OTEL_METRICS_ENABLED=no env var is set', () => {
    process.env.OTEL_METRICS_ENABLED = 'no'

    initOtel()

    expect(getTracer()).not.toBeNull()
    expect(getMeter()).toBeNull()
  })

  it('should disable metrics when OTEL_METRICS_ENABLED=off env var is set', () => {
    process.env.OTEL_METRICS_ENABLED = 'off'

    initOtel()

    expect(getTracer()).not.toBeNull()
    expect(getMeter()).toBeNull()
  })
})
