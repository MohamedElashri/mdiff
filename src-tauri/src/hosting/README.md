# Hosting reviews

The desktop `Hosting` toolbar/palette entry is a lazy frontend module. Provider Markdown uses a separate lazy renderer, without a review-store/worker dependency; raw HTML stays literal and images never fetch. Long comments/reply chains expand on demand. The palette also searches the last discovery page (20 rows) in its Hosting group without network requests; connection changes clear these cached candidates. Standalone palettes omit this group. Provider context lives here rather than in the shared review protocol. Existing pending-review export, PR creation and GitLab draft publishing remain separate flows; hosting discussions never replace local comments.

| Capability | GitHub | GitLab |
| --- | --- | --- |
| Discovery | Open authored / review-requested PRs | Open authored / reviewer-assigned MRs |
| CI | Exact-SHA checks, statuses, workflow runs and numbered attempts | Exact-SHA MR pipelines; project jobs |
| Details | Conversation, paged inline threads with original/revised positions | Notes, paged discussions and positions |
| Discussion writes | Replies, resolve/reopen with viewer permissions | Replies and provider-permitted resolve/reopen |
| Submit | Comment / Approve / Request changes, `commit_id` pinned | Comment with recorded SHA in text; Approve with `sha` |
| Merge | Enabled merge/squash/rebase methods with `sha` | Project-default/squash subject to `user.can_merge` and squash policy, with `sha` |

GitLab approval does not publish the message composer; its text remains a draft. GitLab request-changes, cross-project pipeline job routing, GitHub merge-queue enrollment and auto-merge scheduling are not implemented. A queued/unconfirmed merge is an unknown outcome until reconciliation confirms the merged state. Provider versions, permissions and branch protections can reject an otherwise available action. Missing API fields fail closed for merge capabilities. Local branches are not implicitly associated with remote reviews: open a PR/MR tab to submit actions. GitLab URL subpaths remain unsupported. GitHub threads use fullDatabaseId as text to preserve 64-bit reply identifiers; an older Enterprise schema that lacks it exposes an API error/provider link.

## Authority and lifecycle

`hosting_context` derives the current reference from the native workspace binding and session. `hosting_write` revalidates window ownership, full recorded endpoints, provider head, and diff base while keeping the runtime guard and the workspace lifecycle read gate through the accepted operation and receipt write. Neither renderer-supplied paths nor SHA claims establish authority. GitHub's diff base is computed locally from the fresh PR-associated base SHA and recorded head; missing objects require an explicit review refresh. PR opening fetches that exact API base object rather than assuming it equals today's target branch tip. Submission never starts language servers.

Connection tokens stay in process memory, and are resolved for the exact host. GitHub honors the existing disable preference and `GH_HOST`/`gh` discovery policy; GitLab uses its explicit memory profile. HTTP redirects are disabled for authenticated API calls. GitHub's signed HTTPS job-log download uses a separate request without provider credentials; signed URLs are not cached or exposed in errors.

Settings changes pause reads, cancel the entire multi-request operation, clear authenticated cache data and invalidate the UI. Read cancellation IDs and early-abort tombstones use the existing bounded transport registry, scoped to the originating window. Closing a panel aborts its reads; native close/quit cancels each affected window's reads before waiting on the save barrier. Accepted writes finish independently of their invoking WebView, and close/quit waits for their runtime/lifecycle guards. Each review retains its own ordering; independent reviews have no global write mutex, while atomic ledger preparation prevents duplicate sends.

## Budgets

- Two process-wide hosting network slots across both windows.
- Twenty-second cache TTL and ETag revalidation; coalesced reads keyed by connection epoch, credential digest, host, endpoint and query. No token is stored in keys or on disk.
- At most 32 cached responses / 8 MiB serialized payload, 2 MiB per JSON response; these are payload limits, not a claimed heap/RSS measurement.
- Thirty-second refresh only for the open detail in a visible, focused window. No list or history-row polling. Checks are paged observations, never a combined green badge.
- Twenty rows per page, at most 100 pages; 20 comments per thread. Provider links cover truncated discussions. Write-target thread validation searches at most 200 threads.
- Jobs and up to 100 steps per job load on demand. Log prefixes are bounded at 128 KiB and explicitly marked when truncated. Logs render as text and are never cached.
- Twenty-five-second HTTP deadlines; `Retry-After` seconds/HTTP dates and remaining/reset headers set backoff. No automatic write retries.

## Durable outcomes

`hosting-operations-v1.json` is an atomic, file-locked ledger in app data, capped at 256 records / 1 MiB. A pending receipt is written **before** provider mutation. Records contain the actor, canonical target, reviewed SHA, provider result SHA when reported for a completed action, action metadata, content fingerprint and receipt, never tokens or comment bodies. Pending/unknown entries cannot be evicted to make room.

Operation IDs bind to one exact request. Identical actor/target/SHA/action fingerprints also return the earlier non-rejected receipt, conservatively preventing duplicate clicks across IDs/processes/restarts; intentionally repeating identical text at the same revision requires changed content. Known HTTP client rejection is distinct from transport/server/GraphQL uncertainty. No response or failed receipt persistence is evidence that a write did not happen.

Comments/reviews/replies include an operation marker and reviewed SHA. Reconciliation scans at most 1,000 provider notes/reviews and requires the same authenticated actor. Approval, resolution and merge reconciliation verify current provider state; this confirms the intended result, not necessarily attribution to that specific request. Merge requires the recorded head to match as well as a merged state. An inconclusive result stays unknown with a provider link. GitHub review `commit_id` pins the reviewed commit but does not atomically reject a concurrent push; merge SHA preconditions are provider enforced.

## Validation

Run `make typecheck test-review` and `CARGO_INCREMENTAL=0 make test-native clippy`. Hosting tests use localhost mock providers, including unsafe redirects, wrong CI revisions, credential/cache isolation, cancellation, backoff, payload caps, durable retries, approval/merge request bodies and asynchronous outcomes. Real provider writes are not part of the default suite.

Native smoke testing uses a separately identified debug app and disposable repositories. Check palette/toolbar entry, host switching, missing credentials, disabled GitHub, window close, draft persistence and confirmation presentation. Live account permissions/protections and real submit/merge transactions need separately authorized disposable provider fixtures. Linux native WebKit behavior remains a platform release check.

API references: [GitHub reviews](https://docs.github.com/en/rest/pulls/reviews), [GitHub merges](https://docs.github.com/en/rest/pulls/pulls), [workflow jobs](https://docs.github.com/en/rest/actions/workflow-jobs), [GraphQL review threads](https://docs.github.com/en/graphql/reference/pulls), [GitLab merge requests](https://docs.gitlab.com/api/merge_requests/), [GitLab approvals](https://docs.gitlab.com/api/merge_request_approvals/), [GitLab discussions](https://docs.gitlab.com/api/discussions/).
