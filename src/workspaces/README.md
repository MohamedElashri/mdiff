# Workspaces

Tabs retain independent drafts and navigation. Named workspaces contain ordered tabs and groups. Opening the same source focuses its existing tab in that workspace; other workspaces may keep independent tabs. Saved comments can still share a logical review identity.

## Restore and save

Startup loads metadata; selecting a tab opens its review. At most one heavy review subtree mounts per window. Comparisons default to pinned; working/named-revision requests default to follow. Working text cannot be pinned. Remote tabs restore retained objects offline and untrusted; **Refresh source** explicitly fetches.

Native [persistence](../../src-tauri/src/workspaces.rs) uses atomic writes, file locks and revision checks:

| File                            | Contents / limit                                           |
| ------------------------------- | ---------------------------------------------------------- |
| `workspaces.json`               | Tabs, groups, sources; 4 MiB, 32 workspaces, 128 open tabs |
| `workspace-views/<tab-id>.json` | Drafts/navigation; 2 MiB per tab                           |
| `shell-preferences.json`        | Appearance/shortcuts; 64 KiB                               |

Runtime IDs, credentials and LSP trust never persist. Corrupt/future files are preserved. Stale saves fail rather than overwrite another writer. Autosave waits 650 ms; forced termination can lose recent edits.

Close saves first. Failed saves block close/quit; removing a workspace with unsent drafts is refused. **View unsent drafts** allows copying text without opening its source. Draft-bearing closed tabs are retained; the draft-free reopen list keeps 20. Archives/checkouts have no automatic disk cap.

## Windows

[Native admission](../../src-tauri/src/windows.rs) allows two windows with exclusive workspace ownership. Selecting an owned workspace focuses its window. **Move workspace to new window** saves/drains before transfer. Moving a tab preserves its ID/drafts; duplicating a source creates an independent tab without drafts or trust. Managed duplicates get fresh checkouts without uncommitted edits.

Close drains only its window. Quit/updater restart waits for every window's save acknowledgement and accepted native work; stale acknowledgements cannot approve a later exit. Connection changes retire affected reviews. See [review limits](../../src-tauri/src/reviews.md).
