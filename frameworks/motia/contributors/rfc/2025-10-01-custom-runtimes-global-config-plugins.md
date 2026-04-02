# Custom runtimes, Motia global configuration, Workbench Plugins

This reason this RFC talks about these three topic is because they're related.

## Motia Global Configuration

We propose to add a global configuration file for Motia, similarly to what Vite does, it's flexible
enough to be able to add custom logic like Plugins and Runtimes.

```typescript
import { config } from 'motia'

// Example of Motia official runtimes
import { JavascriptRuntime } from '@motiadev/runtime-javascript'
import { TypeScriptRuntime } from '@motiadev/runtime-typescript'
import { RustRuntime } from '@motiadev/runtime-rust'

// Example of Community runtime
import { JavaRuntime } from '@community/runtime-java'

// Example of a Motia official plugin that doesn't come out of the box
import { mermaidPlugin } from '@motiadev/plugin-mermaid'
import { openApi } from '@motiadev/plugin-open-api'

// Example of Community plugin
import { llmChatPlugin } from '@community/motia-plugin-llm-chat'

export default config({
  runtimes: [
    {
      steps: 'src/**/*.step.ts',
      streams: 'src/**/*.stream.ts',
      runtime: new TypeScriptRuntime(),
    },
    {
      steps: 'src/**/*.step.js',
      streams: 'src/**/*.stream.js',
      runtime: new JavascriptRuntime(),
    },
    {
      steps: 'src/**/*.step.rs',
      streams: 'src/**/*.stream.rs',
      runtime: new RustRuntime(),
    },
    {
      steps: 'src/**/*Step.java',
      streams: 'src/**/*Stream.java',
      runtime: new JavaRuntime(),
    },
  ],

  plugins: [
    // adding mermaid plugin
    mermaidPlugin(),
    openApi({ name: 'My API', description: 'My API description' }),
    llmChatPlugin({ openAiApiKey: process.env.OPENAI_API_KEY }),
  ],

  api: {
    cors: {
      allowedOrigins: ['*'],
      methods: ['GET', 'POST', 'PUT', 'DELETE', 'PATCH', 'OPTIONS', 'HEAD'],
      headers: ['*'],
      credentials: true,
      maxAge: 600,
    },
  },
})
```

In this example above:

- We're configuring three runtimes, one for each language: TypeScript, JavaScript and Rust
  - The `files` option is a glob pattern that matches the step files for each runtime
  - The `runtime` option is a function that returns a new instance of the runtime
- We're configuring three plugins:
  - The `mermaidPlugin` plugin is a plugin that adds mermaid support to the Framework, which means on code changes it will automatically generate a mermaid diagram of the flow, a background plugin that doesn't show up in the workbench.
  - The `openApi` plugin is a plugin that adds OpenAPI support to the Workbench
  - The `llmChatPlugin` plugin is a plugin is an example of a community plugin, in concept: It adds OpenAI support to the Workbench, with a new Tab allowing the user to chat with the LLM about the project.
- We should be able to configure the API cors options

## Motia Plugins

First thing I want to propose, is to rename `LockedData` to `Motia`, which is the representation of the state of the project.

- List of all: Flows, Steps, Streams
- Add watchers to new Steps that are created, deleted or updated

Returns a list of all the components to add to the Workbench. It's possible to override the defaults
of each plugin, like: Label, Placement, and adding props.

### Creating a new plugin

```typescript
class CustomStream extends StreamAdapter<any> {
  constructor(private readonly motia: Motia) {
    super()
  }

  get(id: string): Promise<any> {
    return Promise.resolve(null)
  }

  set(id: string, data: any): Promise<any> {
    return Promise.resolve(data)
  }

  delete(id: string): Promise<any> {
    return Promise.resolve(null)
  }

  getGroup(): Promise<any[]> {
    return Promise.resolve([])
  }
}

export const llmChatPlugin = (args: { openAiApiKey?: string }): MotiaPluginBuilder => {
  return (motia: Motia) => {
    const stream = motia.createStream({
      name: '__motia.custom',
      factory: () => new CustomStream(motia),
    })

    motia.on('flow-created', (flowName) => {
      stream.send({ groupId: 'default', id: flowName }, { type: 'flow-created', data: flowName })
    })

    return {
      /**
       * The Dirname of the plugin, it's important for Motia Framework to know
       * where to find the steps and the workbench components.
       */
      dirname: __dirname,
      /**
       * The steps to load for the plugin.
       */
      steps: ['steps/*.step.ts'],

      /**
       * The workbench components to load for the plugin.
       */
      workbench: {
        {
          packageName: '@community/motia-plugin-llm-chat',

          // optional fields

          /**
           * will ultimately convert to import { LLMChat } from '@community/motia-plugin-llm-chat'
           */
          componentName: 'LLMChat',

          /**
           * The label to display in the workbench
           */
          label: 'Chat with AI',

          /**
           * The placement of the component in the workbench
           */
          placement: 'bottom',

          /**
           * The props to pass to the component
           */
          props: {
            // example of a prop
            disableOpenAi: true,
          }
        },
      },
    }
  }
}
```

### Motia Interface

The Motia interface is the main interface that will be used to interact with the framework.
Features that should be supported day one:

- Add watchers to see new steps, streams, flows being created, updated or deleted
- Create Streams
- Create Steps
- Add custom middlewares to the step execution

## Motia Runtime

The proposal is to move runtimes to be under a Runtime interface.
Motia will orchestrate the runtimes and plugins in one single place.
The framework will handle all the triggers depending on the configuration of each runtime.

```typescript
export interface ReadConfigOptions {
  /**
   * The absolute path to the project root
   */
  projectRoot: string
  /**
   * The absolute path to the step file
   */
  stepFilePath: string
}

export interface BuildResult {
  /**
   * The compressed size of the build in bytes
   */
  compressedSize: number
  /**
   * The uncompressed size of the build in bytes
   */
  uncompressedSize: number
  /**
   * The absolute path to the build in the file system
   */
  path: string
}

export interface ConfigResult {
  /**
   * Is it a step? Then should return a step config
   */
  stepConfig?: StepConfig
  /**
   * Is it a stream? Then should return a stream config
   */
  streamConfig?: StreamConfig

  /**
   * List of all the files that were read to build the config
   * Important to understand whenever the files are changed to
   * rebuild the config and ask for a new compile.
   */
  paths: string[] // relative paths to the project root
}

export interface Runtime {
  name: string

  /**
   * Called whenever the Runtime understands that the step has changed.
   * @param step
   * @returns
   */
  compile: (step: Step) => Promise<void>

  /**
   * Reads the config for a step.
   * @param options
   * @returns
   */
  readConfig: (options: ReadConfigOptions) => Promise<ConfigResult | null>

  /**
   * Calls a step file.
   * @param options
   * @returns
   */
  callStepFile: (step: Step, args?: unknown) => Promise<unknown>

  /**
   * Builds a step.
   * @param step
   * @param destPath
   * @returns
   */
  build(step: Step, destPath: string): Promise<BuildResult>

  /**
   * Builds a consolidated router for a list of api steps.
   * @param steps
   * @param destPath
   * @returns
   */
  buildApiSteps(steps: Step<ApiRouteConfig>[], destPath: string): Promise<BuildResult>
}
```
