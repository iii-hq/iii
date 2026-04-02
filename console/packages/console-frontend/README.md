# Console Frontend

The frontend for the iii Developer Console, built with React 19, TypeScript 5, and Vite 7.

## Quick Start

```bash
# Install dependencies (from monorepo root)
pnpm install

# Start development server
cd packages/console-frontend
pnpm dev
```

The console will be available at **http://localhost:3113**

## Architecture

The console frontend operates in **two modes**:

### Development Mode (Vite)
- Run with `pnpm dev` for active development
- Hot Module Replacement (HMR) enabled
- Direct access to source files
- Connects to iii engine via environment variables

### Binary Mode (rust-embed)
- Built with `pnpm build:binary`
- Assets embedded into Rust binary (`console-rust` package)
- Served directly from the binary via Axum
- Runtime configuration injected via `window.__CONSOLE_CONFIG__`

## Environment Variables

Configure the console to connect to your iii engine instance:

| Variable | Description | Default |
|----------|-------------|---------|
| `VITE_III_ENGINE_HOST` | Engine hostname | `localhost` |
| `VITE_III_ENGINE_PORT` | Engine REST API port | `3111` |
| `VITE_III_WS_PORT` | Engine WebSocket port | `3112` |

### Setting Environment Variables

Create a `.env` file in the `packages/console-frontend` directory:

```bash
VITE_III_ENGINE_HOST=localhost
VITE_III_ENGINE_PORT=3111
VITE_III_WS_PORT=3112
```

Or export them in your shell:

```bash
export VITE_III_ENGINE_HOST=localhost
export VITE_III_ENGINE_PORT=3111
export VITE_III_WS_PORT=3112
```

## Connecting to Remote Engine

To connect to a remote iii engine instance:

```bash
# Option 1: Inline environment variables
VITE_III_ENGINE_HOST=192.168.1.100 pnpm dev

# Option 2: Multiple variables
VITE_III_ENGINE_HOST=192.168.1.100 VITE_III_ENGINE_PORT=4000 pnpm dev

# Option 3: .env file
echo "VITE_III_ENGINE_HOST=192.168.1.100" > .env
echo "VITE_III_ENGINE_PORT=4000" >> .env
pnpm dev
```

## Scripts

| Script | Description |
|--------|-------------|
| `pnpm dev` | Start development server on port 3113 |
| `pnpm dev:standalone` | Start development server with network access (`--host`) |
| `pnpm build` | Build production assets to `dist/` |
| `pnpm build:binary` | Build production assets for Rust binary to `dist-binary/` |
| `pnpm lint` | Run Biome linter on source files |
| `pnpm lint:fix` | Run Biome linter and auto-fix issues |
| `pnpm format` | Format source files with Biome |
| `pnpm preview` | Preview production build locally |

## Troubleshooting

### Connection Refused Errors

**Symptom:** Console shows "Failed to fetch status" or "Connection refused"

**Solutions:**
1. Verify iii engine is running:
   ```bash
   curl http://localhost:3111/_console/status
   ```

2. Check environment variables:
   ```bash
   echo $VITE_III_ENGINE_HOST
   echo $VITE_III_ENGINE_PORT
   ```

3. Ensure ports match your engine configuration:
   - REST API: Default 3111
   - WebSocket: Default 3112

4. For remote connections, verify firewall rules allow access to engine ports

### HMR Not Working

**Symptom:** Changes to source files don't reflect in the browser

**Solutions:**
1. Restart the dev server:
   ```bash
   pnpm dev
   ```

2. Clear Vite cache:
   ```bash
   rm -rf node_modules/.vite
   pnpm dev
   ```

3. Check for TypeScript errors:
   ```bash
   pnpm build
   ```

4. Verify file watcher limits (Linux):
   ```bash
   echo fs.inotify.max_user_watches=524288 | sudo tee -a /etc/sysctl.conf
   sudo sysctl -p
   ```

### Port Already in Use

**Symptom:** `Error: Port 3113 is already in use`

**Solutions:**
1. Kill the process using port 3113:
   ```bash
   # macOS/Linux
   lsof -ti:3113 | xargs kill -9

   # Windows
   netstat -ano | findstr :3113
   taskkill /PID <PID> /F
   ```

2. Use a different port:
   ```bash
   # Edit vite.config.ts and change server.port, or:
   PORT=3114 pnpm dev
   ```

3. Check for zombie Vite processes:
   ```bash
   ps aux | grep vite
   kill <PID>
   ```

### WebSocket Connection Failures

**Symptom:** Real-time updates not working, WebSocket errors in console

**Solutions:**
1. Verify WebSocket port is correct:
   ```bash
   echo $VITE_III_WS_PORT
   ```

2. Test WebSocket connection directly:
   ```bash
   # Use websocat or wscat
   wscat -c ws://localhost:3112
   ```

3. Check browser console for specific WebSocket errors

4. Ensure no proxy or firewall is blocking WebSocket connections

### Build Failures

**Symptom:** `pnpm build` or `pnpm build:binary` fails

**Solutions:**
1. Clean and reinstall dependencies:
   ```bash
   rm -rf node_modules
   pnpm install
   ```

2. Check TypeScript errors:
   ```bash
   pnpm exec tsc --noEmit
   ```

3. Verify Node.js version (requires >= 18):
   ```bash
   node --version
   ```

4. Clear TypeScript build cache:
   ```bash
   rm -rf tsconfig.tsbuildinfo
   ```

## Development Tips

- **Router DevTools:** Available in development mode via TanStack Router DevTools
- **Query DevTools:** Available in development mode via TanStack Query DevTools
- **API Client:** Located in `src/api/client.ts` - handles dual-mode configuration
- **Components:** Reusable UI components in `src/components/ui/`
- **Routes:** File-based routing in `src/routes/` (auto-generates `routeTree.gen.ts`)

## Related Packages

- **console-rust:** Rust binary that embeds and serves this frontend
- **Root:** Monorepo scripts for coordinated development
