# @motiadev/plugin-observability

This package provides React components for the Motia workbench observability functionality.

## Features

- **ObservabilityPage**: Main observability page component with trace timeline and groups
- **TraceTimeline**: Interactive timeline visualization for trace execution
- **TraceGroups**: List of trace groups with filtering and selection
- **TraceDetails**: Detailed view of individual traces with events
- **Event Visualization**: Specialized components for different event types (logs, state, emit, stream)
- **Real-time Updates**: Live updates via stream-client integration

## TailwindCSS Compilation

This package includes TailwindCSS compilation to ensure all Tailwind classes used in the components are properly compiled and available.

### Build Process

The build process includes two steps:

1. **CSS Compilation**: Compiles `src/styles.css` using PostCSS and TailwindCSS
2. **TypeScript Compilation**: Compiles TypeScript files to JavaScript

### Development

- `pnpm run build` - Build both CSS and TypeScript
- `pnpm run dev` - Watch mode for both CSS and TypeScript
- `pnpm run clean` - Clean the dist directory

### Configuration Files

- `src/styles.css` - Main CSS entry point with Tailwind imports

## Components

### Main Components

- **ObservabilityPage**: Main page component with trace timeline and groups
- **TraceTimeline**: Interactive timeline showing trace execution over time
- **TracesGroups**: List view of trace groups with status indicators
- **TraceItemDetail**: Detailed sidebar view of individual traces

### Event Components

- **TraceEvent**: Base component for rendering different event types
- **TraceLogEvent**: Renders log entries with level indicators
- **TraceEmitEvent**: Shows event emissions
- **TraceStateEvent**: Displays state operations
- **TraceStreamEvent**: Visualizes stream operations
- **FunctionCall**: Code-style rendering of function calls

### Utilities

- **useGetEndTime**: Hook for calculating trace end times
- **useObservabilityStore**: Zustand store for managing UI state
