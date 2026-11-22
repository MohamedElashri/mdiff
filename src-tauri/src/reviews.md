# Review ownership and storage

The native [Engine](engine.rs) owns review handles; [frontend scopes](../../vendor/diffle/src/client/runtime.tsx) own transport, stores, DOM controllers and disposal. IPC, events, cancellation and teardown validate the window and runtime ID. Keep registry locks short; serialize mutations per runtime and drain accepted work before teardown.

## Identity and storage

- **Logical review identity** includes the canonical repository/worktree and request, or normalized provider target. Saved discussions, viewed marks and iterations can be shared by matching logical reviews.
- **Tab IDs** own unsent drafts/navigation. **Runtime IDs** are ephemeral window-bound capabilities. Separate reviews never share mutable state, credentials or trust.
- Local/standalone data stays in `<git-dir>/diffle/`. Downloaded reviews use `<app-data>/reviews/<logical-key-hash>/`: `objects.git`, `data/diffle/` and isolated `checkouts/`. Keep data outside disposable checkouts; backups must contain objects, not refs alone or alternates to temporary repositories.
- Preparation and cleanup retain checkout/file leases across processes. Dirty, moved, externally opened or unsuccessfully cleaned checkouts are retained with recovery records. Closing a review never deletes saved data; archives have no automatic disk cap.

## Lifecycle and limits

Eviction checkpoints comparison/view state and pins objects before closing watchers, processes, payloads and LSP. Reconstruction gets a fresh runtime ID and fresh trust; remote objects restore offline, while mutable local reviews resync from disk. Startup restores workspace metadata only. See [workspaces](../../src/workspaces/README.md).

| Resource                                  | Desktop limit                   |
| ----------------------------------------- | ------------------------------- |
| Windows / live reviews / History contexts | 2 each                          |
| Lightweight review handles                | 64                              |
| Git commands / background commands        | 4 / 1                           |
| Retained History walks                    | 2                               |
| Trusted LSP pool / servers                | 1 / 4                           |
| Retained frontend payload estimate        | 48 MiB per window; 96 MiB total |
| Highlight workers                         | 1–2 per WebView; 4 total        |
| Snapshot JSON / combined patch text       | 16 / 32 MiB                     |

These bound retained work, not whole-app RSS. Preserve standalone budgets separately. Credential changes retire affected clients; LSP-command changes revoke trust. Eviction preserves unsent text, and ambiguous legacy drafts require explicit recovery. Further ownership details live in [storage](storage.rs), [workspace persistence](workspaces.rs) and [window admission](windows.rs).
