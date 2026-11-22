# Working on mdiff

mdiff is a Tauri desktop code review app for macOS and Linux, with a React/TypeScript frontend and Rust backend.

## Find the right layer

- `src/`: desktop shell, preferences, integrations, and the review transport adapter.
- `src-tauri/src/`: native commands, review lifecycle, IPC, and OS/hosting integrations.
- `vendor/diffle/src/client/`: shared review UI; `vendor/diffle/crates/diffle/`: shared Rust engine. These are live source used by mdiff. Put shared review changes here and desktop-specific behavior in the shell/adapters.
- UI strings use the shared [localization boundary](vendor/diffle/src/client/i18n/README.md). After changing extracted strings, run `make messages`. Keep translated labels separate from IDs, paths, user content, code and shortcut bindings.
- `vendor/diffle/src/shared/protocol.ts`: wire contract. After changing it, run `make protocol` and include the generated schema change; do not hand-edit `protocol.generated.json`.

Check both desktop and standalone review behavior when changing shared code.

## Build and validate

Use the root [Makefile](Makefile) as the command reference (`make help`), [README.md](README.md#build) for setup requirements, and [workflows](.github/workflows/) for CI coverage.

- `make setup`: install both locked JavaScript dependency trees.
- `make dev`: run the native app; `make dev-web` runs only the frontend and cannot validate native integrations.
- Start with checks for the changed layer: `make typecheck`, `make test-review`, `make test-engine`, or `make test-native`. Find specialized suites in `make help`.
- `make check`: aggregate local validation. Make targets and similarly named npm scripts may cover different checks; inspect their definitions.

Use existing suites for regression coverage. For UI changes, inspect the affected native flow when possible. Report checks run and any validation gaps.

Both Vite builds share a [pinned renderer worker-entry guard](vendor/diffle/scripts/diffs-worker-plugin.ts). Review it and its installed-package regression when updating `@pierre/diffs`; its helper imports must not install window RPC or duplicate worker listeners.

Edit source rather than generated bundles; use Makefile generators for release metadata and license notices. Follow the formatter for the layer you touch; desktop and shared review code use different tools.

## Preserve these boundaries

- History uses a separate read-only repository context and generation-scoped cursors; React cleanup must not close a newer query. Preserve its process/spool limits and shallow-parent semantics; see [history ownership](vendor/diffle/src/client/history/README.md).
- Desktop review requests dispatch through session-scoped Tauri IPC into the engine's in-process router. Preserve review IDs, cancellation, event filtering, and teardown so work cannot leak across reviews.
- Review handles, frontend scopes and managed checkout leases have independent lifetimes. Keep saved remote data outside disposable worktrees, and never use runtime IDs as persistence keys. Preserve owner checks, bounded admission and per-review ref cleanup; see [review ownership](src-tauri/src/reviews.md).
- Worktree and external-tool targets are revalidated in the native owner context, with managed checkout leases retained across launch. Keep renderer paths and arbitrary shell commands out of launch authority; see [worktree/tool ownership](src/worktrees/README.md).
- Desktop Git writes consume native, window/context-bound previews and run as native-owned jobs through the quit barrier. Preserve canonical patch validation, common-dir/index leases, durable uncertain outcomes and external-tool removal protections; see [Git write ownership](src/git-operations/README.md). Read-request cancellation must not abandon an accepted write, and manual Git commands must not receive hosting connection tokens.
- Advanced Git recovery derives state from disk and conflict sides from actual index stages. Keep operation metadata in stale checks, constrain interactive todo commands, and preserve resolution drafts and existing session re-anchoring; see [advanced recovery](src/git-operations/advanced.md).
- Embedded PTYs belong to windows, retain editable checkout leases, and stream through bounded binary credit outside React/review HTTP. Keep terminal input/ACK/close owner checks, explicit startup and all-window shutdown confirmation; see [terminal ownership](src/terminal/README.md).
- Inbox monitoring is process-owned and limited to configured sources; never start review/LSP runtimes or repeat CLI authentication per poll. Preserve incomplete-observation semantics, native destination checks and the explicit tray lifetime; see [attention ownership](src/attention/README.md).
- Agent discovery is opt-in metadata observation, never transcript/argv/environment inspection or authority over unrelated processes. Validate hook peer/start/cwd/session identity and keep agent launches pinned, window-owned and isolated; see [agent authority](src/agents/README.md).
- Language servers require explicit trust for each opened review. Opening a local repository or downloaded PR/MR must not start them automatically or remember trust across reviews.
- Hosting writes validate the native review target/revision and persist an operation receipt before sending. Preserve unknown outcomes and reconcile them instead of automatically retrying; see [hosting authority](src-tauri/src/hosting/README.md).
- Connection tokens use the process-owned native authority in `settings.rs` and are bound to their intended host. Keep them out of settings, Git config, logs, and unrelated hosts.

- Window admission, workspace ownership and the all-window exit barrier belong to [`windows.rs`](src-tauri/src/windows.rs). A capability label pattern is not admission. Preserve generation checks, owner-scoped teardown, and the shared payload/worker budgets when adding windows.
- Workspace metadata and tab views use the native revisioned authority in [`workspaces.rs`](src-tauri/src/workspaces.rs). Keep runtime IDs, credentials and trust out of its files; preserve file leases and compare-and-swap revisions. See [workspace persistence](src/workspaces/README.md) for restore/close limits.

Keep this guide focused on durable decisions and non-obvious traps. Link to maintained sources for changing details, and revise guidance when the architecture or workflow changes.
