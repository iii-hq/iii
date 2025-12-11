# III SDK for Node.js

## Installation

```bash
npm install @iii-dev/sdk
```

## Usage

```javascript
import { Bridge } from '@iii-dev/sdk'

/**
 * Make sure the III Core Instance is up and Running on the given URL.
 */
const bridge = new Bridge(process.env.III_BRIDGE_URL ?? 'ws://localhost:49134')

bridge.registerFunction({ functionPath: 'myFunction' }, (req) => {
  return { status_code: 200, body: { message: 'Hello, world!' } }
})

bridge.registerTrigger({
  triggerType: 'api',
  functionPath: 'myFunction',
  config: { api_path: 'myApiPath', http_method: 'POST' },
})
```

### Registering Functions

III Allows you to register functions that can be invoked by other services.

```javascript
bridge.registerFunction({ functionPath: 'myFunction' }, (req) => {
  // ... do something
  return { status_code: 200, body: { message: 'Hello, world!' } }
})
```

### Registering Triggers

III Allows you to register triggers that can be invoked by other services.

```javascript
bridge.registerTrigger({
  triggerType: 'api',
  functionPath: 'myFunction',
  config: { api_path: 'myApiPath', http_method: 'POST' },
})
```

### Registering Trigger Types

Triggers are mostly created by III Core Modules, but you can also create your own triggers

```javascript
bridge.registerTriggerType(
  {
    /**
     * This is the id of the trigger type, it's unique.
     * Then, you can register a trigger by calling the registerTrigger method.
     */
    id: 'myTriggerType',
    description: 'My trigger type',
  },
  {
    /**
     * Trigger config has: id, functionPath, and config.
     * Your logic should know what to do with the config.
     */
    registerTrigger: async (config) => {
      // ... do something
    },
    unregisterTrigger: async (config) => {
      // ... do something
    },
  },
)
```

### Invoking Functions

III Allows you to invoke functions, they can be functions from the Core Modules or
functions registered by workers.

```javascript
const result = await bridge.invokeFunction('myFunction', { param1: 'value1' })
console.log(result)
```

### Invoking Functions Async

III Allows you to invoke functions asynchronously, they can be functions from the Core Modules or functions registered by workers.

```javascript
bridge.invokeFunctionAsync('myFunction', { param1: 'value1' })
```

This means the Engine won't hold the execution of the function, it will return immediately. Which means the function will be executed in the background.
