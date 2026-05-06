# CLARIFICATION — `iii cloud` user-facing surface

**Blocker for:** [`WORK.cli.md`](./WORK.cli.md) §1, [`WORK.engine.md`](./WORK.engine.md) §1.

## What we know
- `engine/src/main.rs:87` declares `Cloud { args }` ("Manage iii Cloud deployments").
- Dispatches to a separate `iii-cloud` binary.
- The audit confirmed iii Cloud is in scope for ideal-docs (user said "yes in-scope").
- A single-sentence stub now sits on `using-iii/deployment.mdx` ("iii Cloud
  deployments") with a cross-link from `using-iii/cli.mdx`.

## What we need to know

To author beyond the placeholder, we need:

1. **Subcommands.** What does `iii cloud <verb>` accept? Likely candidates: `login`,
   `deploy`, `list`, `status`, `logs`, `rollback`, `destroy`, `link`, `secrets`. Which
   exist or are planned?

2. **Auth model.** How does a user authenticate against iii Cloud? Token-based
   (`iii cloud login`)? OAuth? Stored where (env var, `~/.iii/credentials`,
   keychain)?

3. **Deploy targets.** What does iii Cloud actually run? A managed engine? Managed
   workers? Both? Per-region? Single-region?

4. **Pricing / availability tier.** Is iii Cloud GA, beta, or preview? Affects whether
   docs should warn users about feature flags or pre-release status.

5. **Relationship to local engine.** Does `iii cloud deploy` push the local
   `config.yaml` to a remote engine? Or is the iii Cloud engine separate from the local
   one and configured via its own dashboard?

6. **Doc structure.** Once the surface is known, decide: one cross-cutting page in
   `using-iii/deployment.mdx`, a dedicated `using-iii/cloud.mdx` page, or a separate
   tab? Depends on surface size.

## What to do once clarified

- Replace the placeholder stub on `using-iii/deployment.mdx` with the real section
  outline.
- Add per-subcommand stubs on `using-iii/cli.mdx` if the surface is large.
- If a dedicated page makes sense, create `using-iii/cloud.mdx` and wire into
  `docs.json` under "Using iii".
- Add a `project-rules/cloud.md` if iii Cloud has scoping concerns (e.g., what's iii
  Cloud vs Worker Docs vs ideal-docs).
