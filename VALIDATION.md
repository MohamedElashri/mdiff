# Validation

Run `CARGO_INCREMENTAL=0 make check format-check` for local checks and `make test-performance` for focused workloads. CI runs frontend/engine/native tests, installer tests, and desktop builds. Release builds cover Apple Silicon, Intel macOS, and Linux x86_64.

macOS native and standalone browser checks cover a 1,000-file review, independent tabs/windows, draft restoration, light/dark appearance, compact layouts, French preview, and development pseudo-localization. Linux interactive UI, PTY, tray/notification and accessibility checks remain pending; passing Linux CI does not verify those interactions.

[Review resource limits](src-tauri/src/reviews.md) bound retained work. The macOS debug workload observed roughly 274–802 MiB summed app/helper RSS across cold tabs and two review windows. This is not a controlled release benchmark or a hard memory ceiling. Use `scripts/macos-resource-probe.py` for comparable macOS observations.

OS notification/tray actions, physical keyboard/screen-reader coverage, live provider writes, and paid model execution require separate manual acceptance. French is a partial preview; RTL support is not advertised.

Both Vite builds use the [pinned renderer worker guard](vendor/diffle/scripts/diffs-worker-plugin.ts). Review its installed-package regressions when updating `@pierre/diffs`. Its React development `flushSync` warning remains a dependency limitation.
