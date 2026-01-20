# @motiadev/plugin-streams

A Motia Workbench plugin for visualizing and managing streams in your application.

## Features

- **Stream Discovery**: Automatically discovers all streams defined in your Motia application
- **Real-time Updates**: Shows live stream data as it changes
- **Group Management**: View and manage stream groups
- **Item Inspection**: Drill down into individual stream items with full data viewing
- **Search & Filter**: Quickly find streams by name

## Installation

```bash
npm install @motiadev/plugin-streams
```

## Configuration

Add the plugin to your `motia.config.ts`:

```typescript
import { defineConfig } from 'motia'

export default defineConfig({
  plugins: ['@motiadev/plugin-streams/plugin'],
})
```

## Usage

Once installed, the Streams tab will appear in the Motia Workbench. Click on any stream to view its groups and items.

### Stream Panel

The left panel shows all discovered streams with their names and item counts. Streams are automatically refreshed as data changes.

### Detail View

When you select a stream, the right panel shows:
- Stream metadata (name, schema info)
- Groups within the stream
- Individual items with their data

### Item Inspector

Click on any item to view its full JSON data in a collapsible viewer.

## Development

```bash
# Install dependencies
npm install

# Build the plugin
npm run build

# Watch mode for development
npm run dev
```

## License

Elastic License 2.0 (ELv2)

