# Getting Started

1. Check if iii is installed: `iii --version` (if not, visit https://iii.dev/docs)
2. Run `iii -c iii-config.yaml` to start the server (API on port 3111)
3. Try some curl commands:

   ```bash
   # Create a ticket
   curl -X POST http://localhost:3111/tickets \
     -H "Content-Type: application/json" \
     -d '{"title":"Login issue","description":"Cannot access account","priority":"high","customerEmail":"user@example.com"}'

   # List all tickets
   curl http://localhost:3111/tickets
   ```

4. Explore `/steps` and `config.yaml` to see how Motia works with MultiTriggers and the iii backend

Motia is just the beginning, visit https://iii.dev to learn more and stay up to date with our progress.
