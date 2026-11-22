# Validation

Run `CARGO_INCREMENTAL=0 make check format-check`; use `make test-performance` for focused workloads and `make test-e2e` for Docker-backed browser checks. See [Makefile](Makefile) and [CI workflows](.github/workflows/).

| Coverage                                                  | Status                                                                                                                          |
| --------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| Frontend, engine, native, installer tests; desktop builds | CI on macOS/Linux; release packages for Apple Silicon, Intel macOS and Linux x86_64                                             |
| macOS native + standalone Safari                          | Preferences, review/search/images, history, Git/recovery, workspaces, PTYs, agents and inbox                                    |
| Real agents                                               | Codex and Claude completed reviews; 4 of 10 authorized launch attempts used                                                     |
| Provider publication/merge                                | Mock suites passed; real writes deferred by the user                                                                            |
| Linux interactive acceptance                              | Pending: WebKitGTK, keyboard/accessibility, PTYs/process cleanup, desktop-entry routing, tray/notifications                     |
| Remaining manual checks                                   | OS notification/tray clicks, physical keyboards/screen readers, native color selection, folder drop and terminal selection copy |

The isolated macOS pass covered all theme/font choices, dirty preferences, restart/draft recovery, bounded history paging, local-remote fetch/pull/push/leases, Git previews/cancellation, conflicts, stash/worktree actions, editor routing, Unicode PTYs, agent detection/hooks and real reviews. Shared fixes were checked in rebuilt native and standalone apps.

Latest automated coverage: **572 frontend, 459 engine and 130 native tests passed**; one opt-in live provider-write test remains ignored. Types, lint, strict Clippy, formatting, metadata and packaging checks passed. Docker browser tests could not run because the daemon is unavailable.

Unresolved: the native updater's connection times out before HTTPS although `curl` reaches the manifest. External Terminal/Ghostty UI inspection was denied by computer-use policy; launch receipts passed, completion remains unverified. Parent-thread deletion was not repeated; reply deletion passed.

[Resource limits](src-tauri/src/reviews.md) bound retained work, not total memory. macOS debug observations ranged roughly 274–802 MiB summed app/helper RSS; controlled release/leak profiling remains pending. Use [macOS probe](scripts/macos-resource-probe.py) for comparable observations.

Review the [worker guard](vendor/diffle/scripts/diffs-worker-plugin.ts) and installed-package regressions when updating `@pierre/diffs`. Its development React `flushSync` warning remains an upstream limitation.
