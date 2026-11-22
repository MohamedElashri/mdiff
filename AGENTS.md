# Working on mdiff

mdiff is a Tauri/React/TypeScript/Rust code review app for macOS and Linux.

## Source and checks

- `src/` owns the desktop shell; `src-tauri/src/` owns native integration and IPC.
- `vendor/diffle/src/client/` and `vendor/diffle/crates/diffle/` are live shared UI/engine source. Check desktop and standalone behavior when changing them.
- Use [Makefile](Makefile) targets (`make help`). Start with `make typecheck`, `make test-review`, `make test-engine` or `make test-native`; use existing regression suites.
- `CARGO_INCREMENTAL=0 make check format-check` runs aggregate checks. See [validation gaps](VALIDATION.md) and [CI](.github/workflows/).
- Inspect affected native UI when possible. `make dev-web` cannot validate native integrations. Report checks and gaps.
- Edit source, use each layer's formatter, and generate artifacts through Makefile targets: `make protocol` after wire-contract changes, `make messages` after extracted UI strings, and `make notices TARGET=<triple>` for notices. Never hand-edit bundles or generated schemas.
- When updating `@pierre/diffs`, review the shared [worker guard](vendor/diffle/scripts/diffs-worker-plugin.ts) and installed-package regressions; helper imports must not install window RPC or duplicate listeners.

## Ownership rules

- [Reviews](src-tauri/src/reviews.md): scope IPC, events, cancellation, teardown and private refs to their owner. Runtime IDs are ephemeral; durable data must outlive disposable checkouts.
- [Workspaces](src/workspaces/README.md): preserve stable tab IDs, file leases and revision checks. Window admission and the generation-scoped all-window quit barrier belong to `windows.rs`; capability labels alone grant no admission.
- [History](vendor/diffle/src/client/history/README.md): keep browsing read-only and bounded. Old query cleanup must never close a newer generation; shallow boundaries are not roots.
- [Git writes](src/git-operations/README.md): consume native, window/context-bound previews. Accepted jobs survive request cancellation; preserve Git locks, uncertain receipts and [disk-based recovery](src/git-operations/advanced.md).
- [Tools](src/worktrees/README.md) and [PTYs](src/terminal/README.md): validate native targets and retain checkout leases. Renderer paths or shell commands grant no launch authority. PTYs remain window-owned with bounded streaming.
- [Agents](src/agents/README.md): discovery is opt-in metadata observation. Validate hook peer/start/cwd/session identity; never inspect transcripts, argv or environments or control unrelated processes.
- [Attention](src/attention/README.md): monitor configured sources without starting reviews/LSPs. Incomplete observations cannot prove resolution; validate destinations natively.
- [Hosting](src-tauri/src/hosting/README.md): validate target/revision and persist receipts before writes. Reconcile unknown outcomes; never retry automatically.
- LSP trust is explicit for each opened review and never restored. Tokens stay in native memory, bound to their host; exclude them from files, logs, Git config and manual Git commands.
- [Translations](vendor/diffle/src/client/i18n/README.md) must not change IDs, paths, code, user content or shortcut bindings.
