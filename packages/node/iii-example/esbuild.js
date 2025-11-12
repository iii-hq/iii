const esbuild = require('esbuild')

esbuild
  .build({
    entryPoints: ['src/index.ts'],
    bundle: true,
    sourcemap: true,
    platform: 'node',
    target: ['node22'],
    outdir: 'dist',
    external: [],
  })
  .catch((e) => {
    console.log('Build Failed', e)
    process.exit(1)
  })
