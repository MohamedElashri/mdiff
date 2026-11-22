# Advanced Git and conflict recovery

**Git → Advanced and recovery** supports merge, cherry-pick, revert, linear rebase and constrained interactive rebase. Starting requires a clean checkout on a local branch; merge-commit pick/revert requires an explicit mainline. Fetch remote refs separately before local merge/rebase.

Interactive plans allow reorder, pick, drop, fixup and squash. Every original commit appears once; fixup/squash needs an earlier kept commit. Reword/edit/exec, roots, merge-containing ranges and topology editing require external Git; merge topology is never silently flattened.

## Recovery

Git's on-disk operation metadata and unmerged index are authoritative, including after restart. **Continue** requires resolved entries; **Skip** applies to rebase/pick/revert; **Abort** applies to supported ongoing operations. Unsupported todo instructions disable continue/skip. Stash conflicts have no invented sequencer controls. Skip discards the replayed change; abort cannot undo an already completed operation.

Previews fingerprint operation contents, refs, index and files; changes invalidate them. Interactive todo scripts use native-generated commands/object IDs, never renderer shell text. Execution uses the [same job/receipt framework](README.md): interruptions require inspection, without automatic continuation or rollback.

## Conflict editor

Sides come from actual index stages:

| Operation            | Stage 2: current                       | Stage 3: incoming                 |
| -------------------- | -------------------------------------- | --------------------------------- |
| Merge                | Checkout                               | Merge head                        |
| Rebase               | Rebased destination + replayed commits | Original replayed commit          |
| Cherry-pick          | Checkout                               | Picked commit                     |
| Revert               | Checkout                               | Parent-side inverse               |
| Other unmerged index | Index content                          | Index content; no inferred branch |

Stage 1 is Git's actual base, including synthesized merge bases. Editing requires all three stages, matching regular-file modes, UTF-8/non-NUL text, no filters/encoding/merge drivers, and sides/result ≤64 KiB. Other shapes use external tools.

Choose marker blocks or edit, remove all markers, then preview **Save and stage resolution**. Native execution rejects symlink/hard-link targets and stages only that path under the index lock. Failure can leave edited working text with the original unresolved index.

Revisioned drafts bind checkout/path/stages/operation and never authorize overwriting external edits. Save failures block navigation/close; copying remains available. Accepted jobs refresh related live reviews without clearing composers or changing pinned comparisons. History refs still need explicit refresh. See [implementation](../../vendor/diffle/crates/diffle/src/git/operations/advanced.rs).
