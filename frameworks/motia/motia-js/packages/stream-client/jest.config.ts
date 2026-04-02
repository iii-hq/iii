export default {
  preset: 'ts-jest/presets/default-esm',
  modulePathIgnorePatterns: [],
  resetMocks: true,
  roots: ['__tests__'],
  verbose: true,
  reporters: ['default', ['jest-junit', { outputDirectory: 'reports/unit', outputName: 'unit-test-results.xml' }]],
  testEnvironment: 'node',
  setupFiles: ['dotenv/config'],
  testTimeout: 15000,
  forceExit: true,
  detectOpenHandles: true,
  extensionsToTreatAsEsm: ['.ts'],
  moduleNameMapper: {
    '^(\\.{1,2}/.*)\\.js$': '$1',
    '^uuid$': '<rootDir>/__mocks__/uuid.ts',
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
