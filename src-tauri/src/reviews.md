# Review ownership and storage

The desktop `Engine` owns a bounded registry of review handles. Each handle belongs to
one window label and logical review key; its runtime ID is an ephemeral capability,
not a storage identity. IPC lookup, cancellation, editor operations and close validate
the calling window. Events use that window's emitter and include the runtime ID.
Registry mutexes protect lookup and transitions only. A per-runtime mutex serializes
mutations and teardown; accepted mutations finish even if their UI request disappears.
Per-window preparation locks allow unrelated windows to proceed independently. Native
workspace I/O and teardown participate in a read/write shutdown gate; close drains its
owner before destroying the window, and final exit waits for accepted work.
Per-key opening locks coalesce duplicate opens without serializing unrelated repositories.

## Identity and persistence

Local identity includes canonical common-Git-directory and worktree-Git-directory paths,
plus the source request. GitHub URL identity normalizes the host/repository/PR and adds
local worktree identity when the local remote matches. GitLab identity is origin,
project and MR number. Requests inside managed checkouts use the
archive and request instead of a disposable worktree path. Workspace runtime identities
add a tab-ID namespace; persisted comments retain their logical review identity. Default duplicate
opens within an owner focus the matching live handle. Separate owners may intentionally
share persisted discussions/viewed marks/iterations for the same logical review; their
composers, navigation, async work, auth and LSP trust remain separate.

Local and standalone storage stays in the existing Git-directory `diffle/` location.
Downloaded remote reviews use `<app-data>/reviews/<sha256(logical-key)>/`:

- `objects.git` is a complete bare object database, cloned without hardlinks/alternates.
  Iteration refs alone are never treated as an object backup. Imported public refs live
  separately from each review's UUID-owned comparison refs.
- `data/diffle/` holds indexed SQLite discussions/viewed marks and iteration metadata.
  Review data outlives every checkout. Worktree-specific iteration pin namespaces prevent
  two linked worktrees with a `working` key from deleting one another's objects. Main
  worktree legacy namespaces stay compatible; old linked pins remain recoverable.
- `checkouts/` contains detached, isolated live worktrees. Refreshing/preparing an MR never
  checks out another handle's HEAD. Preparation is locked across processes by logical key.
  History and external-editor operations retain checkout leases.
  Cleanup and reconstruction share preparation admission; reconstruction reuses an
  existing live lease or supersedes a dead lease before its queued cleanup can remove
  the worktree. An OS file lease also excludes other processes from a preferred
  checkout. Workspace checkpoints retain their checkout and avoid asynchronous removal;
  this keeps a published preferred path safe across process exit and restart.

Migration reads the existing cached MR before fetching into a separate staging checkout.
It retains source files, imports SQLite rows transactionally with target edits winning,
and records an import marker so retries cannot resurrect deleted comments. Iteration refs
and metadata copy under the source's iteration lock. Each legacy MR's SHA-based range
data is also aliased into its stable MR range. Individual migrated discussion/viewed
records and iteration metadata are limited to 16 MiB; errors preserve the original data.
Old temporary PR directories already removed by earlier versions cannot be recovered.

Closing/evicting never deletes saved archives or review data. Clean unused worktrees are
removed through Git without `--force`. Dirty/untracked files, moved HEADs, editor-opened
worktrees and failed/timed-out cleanup are retained; `recovery-*.json` beside the archive
records cleanup failures. Inspect those worktrees before removing them with Git. Shutdown
may retain a checkout rather than finish cleanup. Saved archives and retained edits have
no automatic disk-retention cap yet.

## Live and cold lifecycle

A visible handle is active; an invisible live handle is warm. Suspending the React scope
detaches DOM handlers/subscriptions; suspending the native handle makes it eviction-eligible.
Eviction checkpoints the full review/view and logical data key, pins comparison endpoints
and checkout HEAD, then closes watchers, processes, payloads, transport and LSP. Cold
descriptors contain no clients, credentials, shells or language servers. Reconstructing
creates a new runtime ID, reads retained objects offline and starts untrusted. Retained
edited checkouts are reused. Local mutable worktree reviews resync before serving a
snapshot; frozen revision comparisons retain their pinned endpoints and slice.

Closing removes only that owner's handle and its private pins. Window destruction closes
that owner's contexts. App exit drains all contexts. Failed opens and teardown are
idempotent; RAII cleanup keeps admission/checkout leases until owned cleanup finishes.
Workspace descriptors and tab-owned composers/navigation persist through the native
revisioned store; startup restores metadata without opening reviews. Non-workspace engine
descriptors remain process-local. See [desktop workspaces](../../src/workspaces/README.md)
for restore policy, file ownership and metadata limits. Native capabilities cover `main`
and native-created `workspace-*` windows. A separate native registry admits at most two,
projects workspace selection per owner and coordinates the generation-scoped exit barrier.

## Budgets and shared resources

Admission is enforced, with explicit errors when visible work cannot be evicted:

| Resource | Limit / policy |
| --- | --- |
| Native live review runtimes | 2 per Engine; least-recently-used invisible handles evict first |
| Native handle descriptors | 64; close a descriptor before admitting another |
| Lightweight History contexts | 2 total, at most one per owner |
| Retained native History processes | 2 shared by contexts and review sessions; admission times out after 1 second |
| Admitted Git commands | Existing shared scheduler: 4 total, at most 1 background job; History holds a slot while walking |
| Retained cat-file batches | One owner per live repository, separately from the command scheduler; idle batches reap after 2 seconds |
| Trusted LSP pools | 1 per Engine; up to 4 top-level servers; no borrowed trust |
| Session transitions | 256 pending; overflow errors, coalesced watcher work can retry |
| Raw watcher signals | 512; overflow escalates to a full resync |
| Workspace windows | 2 per process, exclusive workspace ownership |
| Frontend live scopes | Desktop: 1 per window; standalone defaults: 2; 64 light navigation descriptors per scope pool |
| Retained frontend payload estimate | Desktop: 48 MiB per window / 96 MiB across two; standalone retains 64 MiB file / 96 MiB pool limits |
| Highlighter workers | One stable provider per desktop WebView, 1–2 workers; at most 4 across two windows |
| Advisory syntax worker | One per WebView; 8 outstanding requests, 8 cached documents / 4M characters |

Snapshot JSON is limited to 16 MiB; combined patch text to 32 MiB. Existing per-repository
patch/cat-file limits still apply and multiply by the two-runtime cap. Frontend API size
checks and history page/spool limits remain additional bounds. Conservative payload
accounting covers retained plain data, not DOM, library caches, JS heap, native RSS,
transient serialization, Git traversal memory or language-server internals. These require
workload measurements; saved drafts are preserved rather than charged to payload eviction.

Raw watchers are shared only for matching canonical worktree/ref targets and debounce
settings. Their invalidations contain no review data/auth. Subscribers keep their own
snapshots and ignored-path filters; the shared watch uses the intersection of ignored
paths. Last-subscriber removal releases native watches and polling tasks. App-owned pin
directories do not feed back into metadata polling.

## Frontend scopes and trust

`createReviewScope` owns API/transport, Zustand review/view/graph stores, DOM root, LSP
target, occurrences/hover/navigation controllers and cancellation/disposal. Production
desktop code never rebinds a global transport. Standalone defaults remain available for
the CLI and existing integrations. Immutable definitions, preferences and highlighting
workers can be shared; exact contents/language validation protects syntax cache reuse.

Desktop workspace composers use native per-tab view files as authority, with
sessionStorage v2 as an in-WebView mirror. Standalone composers use sessionStorage v2
keys with repository/logical identity and composer-owner UUID. Navigation uses serializable descriptors, not callbacks into disposed stores.
Eviction preserves unsent text. Legacy composers lack repository identity, so recovery
requires an explicit banner action; they are never guessed into a repository or allowed
to overwrite an existing v2 draft. The original legacy copy remains available.
If WebView storage rejects a composer write, an owner-keyed memory fallback preserves it
across payload disposal in that WebView. This fallback does not survive app termination.

One native config store publishes changes to every live/warm session. Changes to LSP
commands revoke the old pool and require fresh trust. Credential/hosting-scope changes
invalidate authenticated caches, advance request generations and retire affected clients.
Re-enabling integration requires an explicit reopen. Tokens remain host-bound and in
memory; hashes used to detect changes are also memory-only. Reconstructed cold handles
need explicit reconnection and fresh LSP trust.

Regression coverage lives in native `lifecycle_tests.rs` / `storage.rs`, shared
`tests/{comments,iterations,history,lsp_pool}.rs`, watcher/session tests and frontend
`test/client/runtime.test.ts`. Root Makefile commands cover both desktop and standalone.
