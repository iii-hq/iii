### III Motia

Motia with superpowers!

## Installation

1. Install Motia III Package and Nodemon

```bash
npm install @iii-dev/motia --save-dev
```

2. Install Bun if you haven't already

```bash
brew install bun
```

3. Create a `config.yaml` file in the root of your project and add the following configuration:

```yaml
modules:
  - class: modules::streams::StreamModule
    config:
      port: ${STREAMS_PORT:31112}
      host: 0.0.0.0
      adapter:
        class: modules::streams::adapters::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  - class: modules::api::RestApiModule
    config:
      port: 3111
      host: 0.0.0.0
      cors:
        allowed_origins:
          - http://localhost:3000
          - http://localhost:5173
        allowed_methods:
          - GET
          - POST
          - PUT
          - DELETE
          - OPTIONS

  - class: modules::observability::LoggingModule

  - class: modules::event::EventModule
    config:
      adapter:
        class: modules::event::RedisAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  - class: modules::cron::CronModule
    config:
      adapter:
        class: modules::cron::RedisCronAdapter
        config:
          redis_url: ${REDIS_URL:redis://localhost:6379}

  # Make sure to add the following module to set up the development environment
  - class: modules::shell::ExecModule
    config:
      # watch and extensions should be included only in development mode
      watch:
        - steps/
        - src/
      extensions: ts,js

      # exec is a pipeline of commands, we only need one in production mode
      exec:
        - npx motia dev
        - bun run --enable-source-maps dist/index-dev.js
```

4. Add these scripts to your `package.json` file:

```json
{
  "scripts": {
    "dev": "iii",
    "build": "motia build",
    "start": "iii ---config config-production.yaml"
  }
}
```

- `dev` will start the project in development mode with watchers to reload the project when you make changes to the code.
- `build` will build the project for production.
- `start` will start the project in production mode.

## Note

Before running the project, please be aware that you cannot use **dirname** or **filename** in your code. Motia's build process combines all step files in a single file, so the **dirname** and **filename** will not be valid.
