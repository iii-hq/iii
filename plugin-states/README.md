# @motiadev/plugin-states

This package provides React components for the Motia workbench states functionality.

## TailwindCSS Compilation

This package includes TailwindCSS compilation to ensure all Tailwind classes used in the components are properly compiled and available.

### Build Process

The build process includes two steps:

1. **CSS Compilation**: Compiles `src/styles.css` using PostCSS and TailwindCSS
2. **TypeScript Compilation**: Compiles TypeScript files to JavaScript

### Development

- `pnpm run build` - Build both CSS and TypeScript
- `pnpm run dev` - Watch mode for both CSS and TypeScript
- `pnpm run build:css` - Build only CSS
- `pnpm run dev:css` - Watch mode for CSS only

### Configuration Files

- `src/styles.css` - Main CSS entry point with Tailwind imports

## Features

- **StatesPage**: Main states page component with search, filtering, and bulk delete
- **StateSidebar**: Sidebar component for viewing and editing state details
- **StateDetails**: Read-only JSON view of state values
- **StateEditor**: JSON editor for modifying state values with validation
- **useGetStateItems**: Hook for fetching and managing state items
- **useStatesStore**: Zustand store for managing states UI state
