# Working on mdiff

mdiff is a Tauri desktop code review app for macOS and Linux, with a React/TypeScript frontend and Rust backend.

## Find the right layer

- `src/`: desktop shell, preferences, integrations, and the review transport adapter.
- `src-tauri/src/`: native commands, review lifecycle, IPC, and OS/hosting integrations.
- `vendor/diffle/src/client/`: shared review UI; `vendor/diffle/crates/diffle/`: shared Rust engine. These are live source used by mdiff. Put shared review changes here and desktop-specific behavior in the shell/adapters.
- `vendor/diffle/src/shared/protocol.ts`: wire contract. After changing it, run `make protocol` and include the generated schema change; do not hand-edit `protocol.generated.json`.

Check both desktop and standalone review behavior when changing shared code.

## Build and validate

Use the root [Makefile](Makefile) as the command reference (`make help`), [README.md](README.md#build) for setup requirements, and [workflows](.github/workflows/) for CI coverage.

- `make setup`: install both locked JavaScript dependency trees.
- `make dev`: run the native app; `make dev-web` runs only the frontend and cannot validate native integrations.
- Start with checks for the changed layer: `make typecheck`, `make test-review`, `make test-engine`, or `make test-native`. Find specialized suites in `make help`.
- `make check`: aggregate local validation. Make targets and similarly named npm scripts may cover different checks; inspect their definitions.

Use existing suites for regression coverage. For UI changes, inspect the affected native flow when possible. Report checks run and any validation gaps.

Edit source rather than generated bundles; use Makefile generators for release metadata and license notices. Follow the formatter for the layer you touch; desktop and shared review code use different tools.

## Preserve these boundaries

- Desktop review requests dispatch through session-scoped Tauri IPC into the engine's in-process router. Preserve review IDs, cancellation, event filtering, and teardown so work cannot leak across reviews.
- Language servers require explicit trust for each opened review. Opening a local repository or downloaded PR/MR must not start them automatically or remember trust across reviews.
- Connection tokens stay in memory and are bound to their intended host. Keep them out of settings, Git config, logs, and unrelated hosts.

Keep this guide focused on durable decisions and non-obvious traps. Link to maintained sources for changing details, and revise guidance when the architecture or workflow changes.
