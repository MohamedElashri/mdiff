# Worktrees and external tools

Open **Worktrees and tools** from the toolbar/palette. Browse from a review or lightweight History context without starting a review/LSP. Opening a worktree creates or focuses its working-change tab. [Git operations](../git-operations/README.md) owns creation, removal and other writes.

## Selection and protection

[Native reads](../../vendor/diffle/crates/diffle/src/git/worktrees.rs) use porcelain output with native-byte paths. Only the selected checkout gets status; refresh is explicit, with no fetching/polling. Ahead/behind uses local refs. Bare/prunable entries are unavailable; locked entries remain readable. Inventory is capped at 256 entries / 2 MiB; reads have deadlines and cancellation.

Opaque row IDs belong to a native context. Before status/open/launch, native code re-enumerates worktrees and validates their canonical Git common directory. Renderer paths grant no authority. Managed checkouts retain their owning review/lease; non-UTF-8 paths support inspection/tools but cannot open through review IPC.

External launches retain managed checkouts and record persistent folder protections against app-driven removal. mdiff does not track those processes or release protections automatically: close tools and use external Git to remove their worktrees. [Embedded terminals](../terminal/README.md) instead hold live leases until cleanup.

## Tools

Discovery runs on opening the picker and before launch, without executing probe commands. Missing saved defaults are reported. Launchers pass folders through argv/native folder-open events, set cwd and remove Git location overrides; no folder is interpolated into shell text. Success means launcher acceptance, not confirmed downstream behavior.

[Terminal adapters](../../src-tauri/src/editors/terminals.rs) support Terminal/iTerm on macOS; Ghostty, kitty, Alacritty, WezTerm and xterm; and GNOME Terminal/Console, Xfce Terminal, foot and Konsole on Linux. Each has an explicit folder contract; generic desktop entries/custom shell commands are excluded.

The default terminal is a catalog ID in revisioned `external-tools.json`. Preference edits/discovery never launch a shell. See [validation](../../VALIDATION.md) for platform gaps.
