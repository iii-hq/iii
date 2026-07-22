export const GATE_ROWS = [
  {
    moment: 'pull request',
    integration: 'required deterministic regression',
    quality: 'not the default gate',
    reason: 'contract failures should be fast, reproducible, and credential-free.',
  },
  {
    moment: 'local investigation',
    integration: 'run one fixture or playground',
    quality: 'run the corpus against one subject',
    reason: 'the developer chooses whether the question is contract correctness or agent behavior.',
  },
  {
    moment: 'scheduled evaluation',
    integration: 'confidence preflight',
    quality: 'complete five-scenario run',
    reason: 'real-model variation and cost remain visible over repeated independent runs.',
  },
  {
    moment: 'release candidate',
    integration: 'must be green',
    quality: 'correctness + budget ceilings',
    reason: 'release confidence requires both a healthy substrate and an effective pinned subject.',
  },
  {
    moment: 'baseline comparison',
    integration: 'same contracts',
    quality: 'independent subject reports',
    reason: 'v1 preserves raw dimensions; paired scheduling and aggregate scoring remain outside scope.',
  },
] as const
