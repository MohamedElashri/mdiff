# Validation

Run `CARGO_INCREMENTAL=0 make check format-check`; use `make test-performance` for focused workloads and `make test-e2e` for Docker-backed browser checks. See [Makefile](Makefile) and [CI workflows](.github/workflows/).

| Coverage                                                  | Status                                                                                                      |
| --------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| Frontend, engine, native, installer tests; desktop builds | CI on macOS/Linux; release packages for Apple Silicon, Intel macOS and Linux x86_64                         |
| macOS native + standalone browser                         | 1,000-file review, tabs/windows, drafts, light/dark, compact layouts, French/pseudo-localization            |
| Linux interactive acceptance                              | Pending: WebKitGTK, keyboard/accessibility, PTYs/process cleanup, desktop-entry routing, tray/notifications |
| Other manual acceptance                                   | OS notification/tray actions, physical keyboards/screen readers, live provider writes and model execution   |

[Resource limits](src-tauri/src/reviews.md) bound retained work, not total memory. macOS debug observations ranged roughly 274–802 MiB summed app/helper RSS; controlled release/leak profiling remains pending. Use [macOS probe](scripts/macos-resource-probe.py) for comparable observations.

Review the [worker guard](vendor/diffle/scripts/diffs-worker-plugin.ts) and installed-package regressions when updating `@pierre/diffs`. Its development React `flushSync` warning remains an upstream limitation.
