const commonConfig = {
  preset: 'ts-jest/presets/default-esm' as const,
  resetMocks: true,
  testEnvironment: 'node',
  extensionsToTreatAsEsm: ['.ts'],
  moduleNameMapper: {
    '^(\\.{1,2}/.*)\\.js$': '$1',
  },
  transform: {
    '^.+\\.ts$': [
      'ts-jest',
      {
        useESM: true,
        tsconfig: {
          module: 'ESNext',
          moduleResolution: 'Node',
        },
      },
    ],
  },
}

export default {
  verbose: true,
  projects: [
    {
      ...commonConfig,
      displayName: 'unit',
      testMatch: ['**/__tests__/**/*.test.ts'],
      testPathIgnorePatterns: ['__tests__/integration'],
      testTimeout: 5000,
    },
    {
      ...commonConfig,
      displayName: 'integration',
      testMatch: ['**/__tests__/integration/**/*.test.ts'],
      testTimeout: 15000,
      setupFiles: ['<rootDir>/__tests__/integration/jest-env.ts'],
    },
  ],
}
