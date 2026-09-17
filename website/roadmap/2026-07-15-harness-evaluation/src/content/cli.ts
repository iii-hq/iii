import type { CliTrack } from '@lib/components/diagrams/CliPlayground'

export const CLI_TRACKS: CliTrack[] = [
  {
    id: 'direct',
    label: 'integration',
    lines: [
      {
        cmd: 'make -C harness integration-validate',
        out: ['✓ 4 fixtures compiled', '→ E2E-001 · E2E-002 · UI-001 · UI-002'],
      },
      {
        cmd: 'make -C harness integration-e2e III_BIN=/path/to/iii',
        fn: 'harness::send',
        out: ['✓ E2E-001 streamed-text · pass', '✓ E2E-002 exactly-once-function · pass'],
      },
      {
        cmd: 'jq . target/integration/*/result.json',
        out: [
          '{ "classification": "pass", "schema_version": "1" }',
          '→ execution.json links the exact bytes by SHA-256',
        ],
        exit: 0,
      },
    ],
  },
  {
    id: 'quality',
    label: 'quality / e2e',
    lines: [
      {
        cmd: 'launcher → harness-agent-quality',
        out: ['✓ subject file validated', '→ model · provider · prompt · skills · harness · workers pinned'],
      },
      {
        cmd: 'run the five-scenario corpus',
        out: ['→ plain response · single function · security review', '→ triggered work · functional reduction'],
      },
      {
        cmd: 'inspect correctness + benchmark',
        out: [
          '✓ durable outcome and evidence checks passed',
          '{ "tokens": "reported", "cost": "reported", "turns": "reported" }',
        ],
        exit: 0,
      },
    ],
  },
]
