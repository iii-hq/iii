// Generates the OTLP/JSON fixtures the Go serializer tests compare against, using the
// Node SDK's own serializers (@opentelemetry/otlp-transformer JsonTraceSerializer,
// JsonLogsSerializer and JsonMetricsSerializer, the version pinned by
// sdk/packages/node/helpers/package.json). The inputs below are the same spans, log
// records and metrics otlpjson_test.go builds with the Go SDK.
//
// Usage (any node_modules directory that holds @opentelemetry/otlp-transformer and its
// @opentelemetry siblings, e.g. a pnpm store entry):
//
//   OTLP_NODE_MODULES=/path/to/node_modules node generate.cjs
//
// It rewrites traces.json, logs.json and metrics.json next to this file.
'use strict'

const fs = require('node:fs')
const path = require('node:path')

const dir = process.env.OTLP_NODE_MODULES
if (!dir) {
  console.error('set OTLP_NODE_MODULES to a node_modules directory containing @opentelemetry/otlp-transformer')
  process.exit(1)
}
const t = require(path.join(dir, '@opentelemetry/otlp-transformer'))
const { DataPointType, AggregationTemporality } = require(path.join(dir, '@opentelemetry/sdk-metrics'))
const { ValueType } = require(path.join(dir, '@opentelemetry/api'))

const version = require(path.join(dir, '@opentelemetry/otlp-transformer/package.json')).version

// Go's attribute sets are sorted by key; the JS objects list keys in that order so the
// documents can be compared byte for byte.
const resource = {
  attributes: { 'service.instance.id': 'inst-1', 'service.name': 'svc', 'service.version': '1.2.3' },
}
const scope = { name: 'svc' }

const traceId = '4bf92f3577b34da6a3ce929d0e0e4736'

const spans = [
  {
    name: 'execute orders::create',
    kind: 0, // INTERNAL
    spanContext: () => ({ traceId, spanId: '00f067aa0ba902b7', traceFlags: 1 }),
    parentSpanContext: { traceId, spanId: 'b7ad6b7169203331', traceFlags: 1, isRemote: true },
    startTime: [1700000000, 123],
    endTime: [1700000001, 456],
    attributes: { tenant: 'acme', n: 42, ratio: 0.5, ok: true, tags: ['a', 'b'] },
    droppedAttributesCount: 0,
    events: [
      {
        name: 'iii.invocation.input',
        time: [1700000000, 500],
        attributes: { 'iii.payload.json': '{"a":1}', 'iii.payload.truncated': false },
        droppedAttributesCount: 0,
      },
    ],
    droppedEventsCount: 0,
    status: { code: 2, message: 'boom' }, // ERROR
    links: [],
    droppedLinksCount: 0,
    resource,
    instrumentationScope: scope,
  },
  {
    name: 'GET /api',
    kind: 2, // CLIENT
    spanContext: () => ({ traceId, spanId: '2222222222222222', traceFlags: 1 }),
    parentSpanContext: undefined,
    startTime: [1700000002, 0],
    endTime: [1700000002, 999999999],
    attributes: { 'http.request.method': 'GET', 'server.port': 8080 },
    droppedAttributesCount: 0,
    events: [],
    droppedEventsCount: 0,
    status: { code: 1 }, // OK
    links: [
      {
        context: { traceId, spanId: '1111111111111111', traceFlags: 1, isRemote: true },
        attributes: { k: 'v' },
        droppedAttributesCount: 0,
      },
    ],
    droppedLinksCount: 0,
    resource,
    instrumentationScope: scope,
  },
]

const logs = [
  {
    hrTime: [1700000000, 1000],
    hrTimeObserved: [1700000000, 2000],
    severityNumber: 9, // INFO
    body: 'Order processed',
    attributes: {
      trace_id: traceId,
      span_id: '00f067aa0ba902b7',
      'log.data': { amount: 49.99, count: 3, nested: { ok: true }, orderId: 'ord_123', tags: ['x'] },
    },
    droppedAttributesCount: 0,
    spanContext: { traceId, spanId: '00f067aa0ba902b7', traceFlags: 1 },
    resource,
    instrumentationScope: scope,
  },
  {
    hrTime: [1700000003, 0],
    hrTimeObserved: [1700000003, 5],
    severityNumber: 17, // ERROR
    body: 'no span',
    attributes: {},
    droppedAttributesCount: 0,
    resource,
    instrumentationScope: scope,
  },
]

const start = [1700000000, 0]
const end = [1700000060, 0]
const resourceMetrics = {
  resource,
  scopeMetrics: [
    {
      scope,
      metrics: [
        {
          descriptor: { name: 'iii.worker.memory.heap_used', description: 'Worker heap memory used in bytes', unit: 'bytes', valueType: ValueType.DOUBLE },
          aggregationTemporality: AggregationTemporality.CUMULATIVE,
          dataPointType: DataPointType.GAUGE,
          dataPoints: [{ attributes: { 'worker.id': 'w-1', 'worker.name': 'orders' }, startTime: start, endTime: end, value: 1048576.5 }],
        },
        {
          descriptor: { name: 'requests', description: '', unit: '1', valueType: ValueType.INT },
          aggregationTemporality: AggregationTemporality.CUMULATIVE,
          dataPointType: DataPointType.SUM,
          isMonotonic: true,
          dataPoints: [{ attributes: { route: '/a' }, startTime: start, endTime: end, value: 7 }],
        },
        {
          descriptor: { name: 'latency', description: 'd', unit: 'ms', valueType: ValueType.DOUBLE },
          aggregationTemporality: AggregationTemporality.DELTA,
          dataPointType: DataPointType.HISTOGRAM,
          dataPoints: [
            {
              attributes: {},
              startTime: start,
              endTime: end,
              value: { buckets: { boundaries: [0, 5, 10], counts: [1, 2, 0, 1] }, count: 4, sum: 20.5, min: 0.5, max: 12 },
            },
          ],
        },
      ],
    },
  ],
}

const decode = (bytes) => Buffer.from(bytes).toString('utf8')
const out = {
  'traces.json': decode(t.JsonTraceSerializer.serializeRequest(spans)),
  'logs.json': decode(t.JsonLogsSerializer.serializeRequest(logs)),
  'metrics.json': decode(t.JsonMetricsSerializer.serializeRequest(resourceMetrics)),
}
for (const [file, body] of Object.entries(out)) {
  fs.writeFileSync(path.join(__dirname, file), body + '\n')
}
console.log(`wrote ${Object.keys(out).join(', ')} with @opentelemetry/otlp-transformer ${version}`)
