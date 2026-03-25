import path from 'node:path'
import { tanstackRouter } from '@tanstack/router-plugin/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig(({ mode }) => ({
  plugins: [
    tanstackRouter({
      target: 'react',
      routesDirectory: './src/routes',
      generatedRouteTree: './src/routeTree.gen.ts',
      routeFileIgnorePrefix: '-',
      quoteStyle: 'single',
    }),
    react(),
  ],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  build: {
    outDir: mode === 'binary' ? 'dist-binary' : 'dist',
    sourcemap: false,
    rollupOptions: {
      output: {
        manualChunks: {
          'react-vendor': ['react', 'react-dom'],
          router: ['@tanstack/react-router'],
          query: ['@tanstack/react-query'],
          'ui-icons': ['lucide-react'],
          'ui-components': ['@radix-ui/react-tabs', '@radix-ui/react-dialog', '@radix-ui/react-slot', '@radix-ui/react-select', '@radix-ui/react-label', '@radix-ui/react-separator', 'class-variance-authority', 'cmdk', 'sonner'],
        },
      },
    },
  },
  server: {
    port: 3114,
    proxy: {
      '/api': 'http://localhost:3113',
    },
  },
}))
