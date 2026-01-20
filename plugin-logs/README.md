# @motiadev/plugin-logs

This package provides React components for the Motia workbench logs functionality.

## TailwindCSS Compilation

This package includes TailwindCSS compilation to ensure all Tailwind classes used in the components are properly compiled and available.

### Build Process

The build process uses Vite with integrated CSS compilation:

1. **Vite Build**: Compiles TypeScript, CSS, and generates type declarations
2. **Output Formats**: Generates both ES modules and CommonJS formats
3. **Type Declarations**: Automatically generates `.d.ts` files

### Development

- `pnpm run build` - Build all files using Vite
- `pnpm run dev` - Watch mode for development
- `pnpm run clean` - Remove the dist directory

### Configuration Files

- `vite.config.ts` - Vite configuration with TailwindCSS plugin
- `src/styles.css` - Main CSS entry point with Tailwind imports

## Features

- **LogsPage**: Main logs page component with search and filtering
- **LogDetail**: Sidebar component for viewing detailed log information
- **LogLevelBadge**: Badge component for log levels
- **useLogsStore**: Zustand store for managing logs state
