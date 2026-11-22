# Desktop workspaces

The desktop shell owns tabs and named workspaces. The shared review client still works
standalone. `model.ts` sends operations to `src-tauri/src/workspaces.rs`; it never stores
the authoritative workspace document in localStorage. `WorkspaceBar.tsx` is lazy and
uses the existing theme and compact controls.

## Durable identities

A stable tab ID owns its composers and navigation. A native runtime ID is ephemeral,
window-scoped and never saved. A workspace contains ordered tabs, its active tab,
recent sources and named/color/collapsed groups. Opening the same canonical local
source/request in a workspace focuses its existing tab. Another workspace can keep
its own tab for that source. Saved comments/iterations still share the engine's logical
review identity; unsent composers remain tab-specific.

Native config files:

- `workspaces.json`: version 1, monotonic revision, at most 4 MiB; 32 workspaces,
  128 open tabs, 16 groups per workspace, 12 recent sources per workspace.
- `workspace-views/<tab-id>.json`: independent revision, at most 2 MiB; unsent text
  (at most 1,000 entries) and light navigation (at most 128 KiB). The UI limits optional
  navigation to roughly 64 KiB, retaining at most 24 KiB of collapse exceptions.
- `shell-preferences.json`: appearance, theme and shortcut overrides; version/revision,
  at most 64 KiB. localStorage remains a compatibility mirror for shared client APIs.
- `workspace-leases/<tab-id>`: an OS file lock excludes other processes from editing
  an activated tab's view or closing/reconfiguring it. Unload releases ownership.

Writers use cross-process locks, fresh reads, revision checks and atomic replacement.
A stale writer receives an error and reloads; it does not overwrite another writer.
Events coordinate admitted WebViews in one process. The native window registry projects
its own active workspace into each window's response; another window's selection never
replaces it. Other app processes discover revision
changes on their next read/write; there is no external-file polling. Corrupt/future
files produce an error and remain intact. Migration imports legacy desktop recents
and preferences once without deleting originals. A tab may adopt its exact Phase 4
repository-owned composers once; the adoption marker is written only after native
persistence succeeds. Ambiguous older composers retain the explicit recovery flow.

## Restore and lifecycle

Startup reads metadata only: no Git probes, checkout preparation, review runtimes,
workers or language servers for cold tabs. Selecting a tab opens it on demand. At most
one heavy review subtree is mounted in each window. Two native runtimes remain the
process-wide limit. Desktop scopes retain one live payload per window, limited to
48 MiB each, so two windows stay within the established 96 MiB aggregate estimate. Native cold
workspace handles are reaped once durable metadata can reconstruct them. The shared
highlighter provider stays stable across switching and Home.

Revision comparisons default to pinned; local working and named-revision requests
default to follow. Checkpoints save both original named intent and frozen endpoints,
plus the comment/iteration identity and current slice. Pinning freezes a live range
in place. Follow resolves the original request on reconstruction and reports changed
comparison endpoints; working changes resync before serving. Working text cannot be
pinned. Remote tabs restore retained objects offline and untrusted; Refresh source
explicitly re-fetches using current in-memory credentials. Connection clients and
trust never enter metadata. Changing review mode derives a new comment key rather
than carrying the reconstruction override into another mode.

Workspace checkpoint refs live under
`refs/diffle/iterations/workspaces/<tab>/<generation>/`. A new generation is pinned
before publication; old generations retire after metadata commits. Managed source
objects/data live outside temporary paths. Published workspace checkouts are retained,
and preferred-checkout reuse acquires an OS file lease under the archive's `leases/`
directory. Another process must release that checkout before it can be reused. A
missing clean checkout is reconstructed from retained objects. Retention avoids
background removal outliving a workspace checkout lease; there is no automatic disk
cap for these checkouts or saved archives. Inspect them before manual Git cleanup.

Selection/line drafts/scroll restore only against matching revision and file blob
identities. Full-file navigation saves a path and return location, never the loaded
payload. External LSP files are not reopened automatically. Graph pan/zoom/selection
are retained, but a cold review still requires explicit trust before code intelligence
can run. Unsent text remains available even when its original line anchor disappears;
Tab options → View unsent drafts allows copying it without reopening its source.

Close saves before moving a tab to the reopen list. Keep the most recent 20 draft-free
closed tabs; retain draft-bearing records within the explicit metadata bound. Removing
a workspace with unsent drafts is refused. Neither action deletes repositories,
archives or comments. Window close, quit and updater restart ask the shell to flush
view/preference writes, then wait for accepted activation/engine operations. Failed
persistence keeps the window open with an error. Autosave is debounced 650 ms: force
termination can lose edits made since the last successful write. Source preparation
can delay graceful exit until its existing operation deadline.

## Workspace windows

The native `windows.rs` registry admits at most two windows per process. Capabilities
cover `main` and native-created `workspace-*` labels; admission is checked separately
from that label pattern. Each window owns one workspace, its runtime/History contexts,
events, requests and cancellation. Selecting a workspace already owned by another
window focuses that window. Closing the original `main` window leaves an admitted
workspace window operational. Restart restores metadata in one cold window; reopen
additional windows explicitly rather than allocating their WebViews at startup.

In Manage workspaces, **Move workspace to new window** saves the current view and
preferences, drains the source's review/History contexts and transfers ownership. The
source selects an available workspace or creates an empty one. Tab options offer:

- **Move tab to …**: preserve its stable ID and native draft/navigation files, close
  its old runtime, then make it available in the other window. The destination's
  active review is undisturbed until the reader selects the moved tab.
- **Duplicate source in new window**: create a workspace and independent tab, with
  new endpoint pins, no unsent text, no trust and no credentials in its files. Saved
  comments still share the logical review identity. Local copies see the same local
  working directory; a managed duplicate receives a fresh checkout without copying
  the original checkout's uncommitted edits. Duplication never starts a runtime eagerly.

Window creation is serialized with workspace operations and respects the two-window
cap before allocating a WebView. Failed creation preserves saved records. Repository preparation and tab saves serialize
within their owner; an unrelated window's preparation proceeds independently. A short
structural transaction coordinates workspace claims, and an app-wide read/write gate
lets accepted native work finish before final shutdown. Native
runtime admission stays at two, with one global LSP pool; two visible reviews do not
leave capacity for a third warm runtime. Each desktop WebView admits one review
payload and 1–2 highlighting workers (at most four across both windows); each also
has its existing bounded advisory syntax worker. Standalone review pool defaults
remain unchanged. These quotas cover retained payloads/workers, not whole-process RSS,
WebKit caches or transient serialization.

Connection profiles live in a process-owned native authority with revision checks.
Preferences load the current profile; Save rejects stale tool/connection edits from
another window. All new reviews obtain the native profile rather than a renderer's
cached token. Changes revoke affected clients, and affected mounted reviews save and
return Home before reopening. Hosts remain explicitly bound to their tokens. Appearance,
color mode and shortcuts continue using their cross-process native file revisions.
There is no token persistence and trust never transfers with a window or tab.

Close flushes and drains only that window's contexts before destroying it. Its flush
event uses a WebView-specific listener and carries an explicit owner label; global
listeners must not treat another window's close as their own. Quit and updater restart use a generation-scoped
all-window barrier: every registered renderer saves its view/preferences and waits for
active update work; a failed save cancels the exit for all windows. Stale acknowledgements
cannot approve a later quit. Accepted native work drains before process shutdown.
Update check/download/install admission has one owner across windows. Forced process
termination still has the autosave limitation described above.
