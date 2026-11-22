# Embedded terminal

Open **Tools → Embedded terminal → Start shell**, or start one from **Worktrees and external tools**. Configure startup, font size, and preferred external terminal in **Preferences → Terminal & tools**. Each window allows two tabs; the process allows four live PTYs.

## Ownership

[Native startup](../../src-tauri/src/terminal/mod.rs) accepts an owned review/History context and opaque worktree ID, never a renderer path/executable. The creating window owns the shell and retains its launch checkout until cleanup. Switching/closing reviews does not move or stop it. The displayed path is the launch directory; later `cd` locations are not protected.

Live tab/window close and quit/restart require confirmation. Cancelled quit preserves shells; accepted quit waits for every window. WebView teardown stops owned terminals. Commands/output never restore after restart. Managed checkouts become editable and are retained conservatively.

The interactive login shell uses absolute `$SHELL`, falling back to `/bin/zsh` on macOS or `/bin/bash` on Linux. Startup strips Git injections, askpass overrides, known hosting-token variables and Rex IDs; app credentials are never injected. Shell login files, user credentials and shell-managed history still apply. mdiff does not persist terminal I/O.

## Streaming and shutdown

[PTY core](../../src-tauri/src/terminal/core.rs) and [controller](controller.ts) use binary channels outside review HTTP/React state. Four 8 KiB output frames wait for cumulative xterm parse acknowledgements; reading then stalls. Input is bounded to 64 KiB and never retried automatically. Ownership/sequence checks reject invalid commands/frames. Scrollback is 1,000 lines; privileged clipboard/notification/link escapes cannot trigger app actions.

Stop sends TERM to the Unix session, escalates after 250 ms, reaps the shell and releases leases. Natural exit drains output; explicit stop may discard the tail. Daemonized processes and force-kill are outside normal cleanup guarantees.

The macOS [termination bridge](../../src-tauri/src/windows/macos.rs) routes Cmd+Q/Dock exit through confirmation. Revisit it when upgrading Tao/AppKit delegation; logout/shutdown may need repeating after mdiff exits. See [validation](../../VALIDATION.md).
