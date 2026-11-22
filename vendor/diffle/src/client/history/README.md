# Repository history

[history::Repository](../../../crates/diffle/src/history.rs) owns read-only repository identity and one pinned query, separate from review sessions, credentials, watchers and LSP. Desktop contexts retain managed-checkout leases; standalone reuses CLI identity.

## Queries and limits

Queries capture HEAD/branch/tag object IDs, excluding app/replacement refs. No lazy network fetch occurs. Refresh creates a generation; missing objects are reported. **Close is generation-specific**: old React cleanup must never close a newer query.

| Resource          | Limit                                                                            |
| ----------------- | -------------------------------------------------------------------------------- |
| Traversal         | One `git log --topo-order` process per query; two retained walks shared natively |
| Page              | 100 matches / 5,000 examined commits                                             |
| Query             | 200,000 examined commits; 32 MiB temporary spool                                 |
| Lifetime          | 10-second call deadline; 30-second idle expiry                                   |
| Browser retention | Ten pages / 1,000 rows; 8 MiB serialized payload                                 |

Cursors identify a generation/page; replay returns that exact page. Cancellation releases the process, permit and spool; completed walks retain replayable pages. Leaving History stops traversal, retaining only the visible window; Reload starts fresh. Payload/spool limits do not bound Git memory or JS heap.

## Graph and comparisons

Keep 36 px rows aligned with the virtualizer. Lanes cap at 32; mark excess/shallow boundaries instead of inventing edges. Fold only verified, fully loaded linear side branches joining a known spine; filtering must not imply ancestry. Full messages/trees load on demand.

Comparisons validate full object IDs/actual parents and preserve endpoint order. Series require a newest-first contiguous first-parent chain, at most 250 commits. Roots use the engine's empty-tree endpoint; shallow boundaries are not roots. History focus suspends review commands while app commands remain available.

Create a disposable wide-history fixture from the root:

```sh
python3 vendor/diffle/scripts/history-probe.py /tmp/mdiff-history --create --commits 50000
```

The probe refuses existing destinations. Measure real API paging/replay/cancellation and memory, not traversal alone. See [review ownership](../../../../../src-tauri/src/reviews.md).
