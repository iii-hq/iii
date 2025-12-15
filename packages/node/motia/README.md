### III Motia

Motia with superpowers!

## Installation

1. Install Motia III Package and Nodemon

```bash
npm install @iii-dev/motia nodemon --save-dev
```

2. Create a `nodemon.json` file in the root of your project and add the following configuration:

```json
{
  "watch": ["steps", "src"],
  "ext": "ts,js",
  "exec": "motia dev && bun run --enable-source-maps dist/index.js",
  "quiet": true
}
```

3. Install Bun if you haven't already

```bash
brew install bun
```

4. Add these scripts to your `package.json` file:

```json
{
  "scripts": {
    "dev": "nodemon --config nodemon.json",
    "build": "motia build",
    "start": "bun run --enable-source-maps dist/index-production.js"
  }
}
```

- `dev` will start the project in development mode with watchers to reload the project when you make changes to the code.
- `build` will build the project for production.
- `start` will start the project in production mode.

## Note

Before running the project, please be aware that you cannot use **dirname** or **filename** in your code. Motia's build process combines all step files in a single file, so the **dirname** and **filename** will not be valid.
