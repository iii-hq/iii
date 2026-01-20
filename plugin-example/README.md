# {{PROJECT_NAME}}

A minimal example plugin demonstrating the Motia plugin system.

## Overview

This plugin serves as a reference implementation showing how to create custom workbench plugins for Motia. It demonstrates:

- Basic plugin structure and configuration
- Creating custom workbench tabs with position control
- Using Motia's UI component library (`@motiadev/ui`)
- Building with tsdown and TypeScript
- Tailwind CSS v4 styling with PostCSS
- React Compiler optimization via Babel

## Installation

```bash
pnpm install
```

## Development

```bash
# Build the plugin
pnpm run build

# Watch mode for development
pnpm run dev

# Clean build artifacts
pnpm run clean
```

## Usage

To use this plugin in your Motia project, import it in your `motia.config.ts`:

```typescript
import examplePlugin from '{{PROJECT_NAME}}/plugin'

export default {
  plugins: [examplePlugin],
}
```

## Plugin Configuration

The plugin exports a function that receives the `MotiaPluginContext` and returns a `MotiaPlugin` object:

```typescript
import type { MotiaPlugin, MotiaPluginContext } from '@motiadev/core'

export default function plugin(_motia: MotiaPluginContext): MotiaPlugin {
  return {
    workbench: [
      {
        packageName: '@motiadev/plugin-example',
        cssImports: ['@motiadev/plugin-example/dist/index.css'],
        label: 'Example',
        position: 'bottom',
        componentName: 'ExamplePage',
        labelIcon: 'sparkles',
      },
    ],
  }
}
```

### Workbench Options

| Option          | Description                                |
| --------------- | ------------------------------------------ |
| `packageName`   | The npm package name for dynamic imports   |
| `cssImports`    | Array of CSS files to load with the plugin |
| `label`         | Display name shown in the workbench tab    |
| `position`      | Tab position: `'top'` or `'bottom'`        |
| `componentName` | Name of the exported React component       |
| `labelIcon`     | Lucide icon name for the tab               |

## Structure

```
{{PROJECT_NAME}}/
├── src/
│   ├── components/
│   │   └── example-page.tsx    # Main UI component
│   ├── index.ts                # Package entry point (exports components)
│   ├── plugin.ts               # Plugin definition
│   └── styles.css              # Tailwind CSS styles
├── dist/                       # Build output
├── package.json
├── tsconfig.json
├── tsdown.config.ts            # Build configuration
├── postcss.config.js           # PostCSS/Tailwind config
└── README.md
```

## Features Demonstrated

- **Custom Workbench Tab**: Adds an "Example" tab to the bottom panel
- **UI Components**: Uses `Badge` and `Button` from `@motiadev/ui`
- **Icons**: Integrates Lucide React icons
- **Responsive Layout**: Grid-based responsive design with Tailwind CSS
- **Type Safety**: Full TypeScript support with proper type declarations

## Learn More

For detailed documentation on creating plugins, see the [Plugins Guide](../../packages/docs/content/docs/development-guide/plugins.mdx).
