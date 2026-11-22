# Git operations

Open **Git** from the toolbar/palette. Use a local review or **Choose repository**, including an unborn repository. App-managed PR/MR checkouts require a user-owned clone for these writes. Browsing starts no language servers.

| Area         | Supported actions / limits                                                                                                        |
| ------------ | --------------------------------------------------------------------------------------------------------------------------------- |
| Repositories | Init/clone into a new folder; partial failures remain for inspection                                                              |
| Refs         | Create/switch/delete branches, lightweight tags, remotes; switch requires clean state, branch deletion requires merged/unoccupied |
| Changes      | File stage/unstage; canonical text-hunk stage/unstage; unsupported patches use whole-file actions                                 |
| Commits      | Commit/amend staged paths; installed hooks/signers run; interactive prompts need external Git                                     |
| Restore      | Restore one file from index; permanently discard one untracked regular file; no recursive deletion                                |
| Network      | Fetch, fast-forward-only pull, exact-ref normal/lease push                                                                        |
| Worktrees    | Create, lock/unlock, remove clean unused checkouts, prune missing registrations; no force removal                                 |
| Stashes      | Save, apply, latest pop/drop; conflicts retain the stash                                                                          |

Manual network actions use installed Git credentials/helpers and SSH agent, never mdiff hosting tokens. Destinations accept constrained HTTPS/SSH/local transports; arbitrary helpers and embedded HTTPS credentials are rejected. Confirmed writes may run configured hooks, filters and signers. Read previews suppress those programs, so filter-managed files may appear modified.

## Preview, execution and recovery

[Native commands](../../src-tauri/src/git_operations/mod.rs) own typed, single-use, window/context-bound previews. Execution rechecks checkout identity, HEAD/refs/config/index and affected content. Common-directory/index leases supplement Git locks; ref updates/pushes use expected object IDs. External writers can still race.

Accepted jobs survive IPC cancellation/window close; quit drains them. Explicit cancellation terminates/reaps the process group. A pending [receipt](../../src-tauri/src/git_operations/ledger.rs) precedes execution. Failed/interrupted writes show **Needs inspection**: no replay, automatic retry or rollback. Inspect actual state and retained partial destinations.

Commit drafts save separately with revision checks; save failures preserve text and block close. Catalogs refresh explicitly; History needs **Refresh refs** after writes. Reads/output/jobs are bounded in the [native adapter](../../src-tauri/src/git_operations/mod.rs) and [shared engine](../../vendor/diffle/crates/diffle/src/git/operations/).

Ongoing operations block everyday writes. Use [Advanced and recovery](advanced.md) for supported recovery. [Worktree/tool leases](../worktrees/README.md) prevent removing in-use checkouts. Desktop writes require UTF-8 paths; standalone review has no mutation routes.
