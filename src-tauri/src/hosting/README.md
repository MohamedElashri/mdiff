# Hosting reviews

Open **Hosting** for review discovery, conversations and revision-specific CI. Provider discussions remain separate from local comments. Markdown keeps raw HTML literal and never fetches images; logs load on demand as text, capped at 128 KiB.

| Action   | GitHub                                        | GitLab                                  |
| -------- | --------------------------------------------- | --------------------------------------- |
| Discover | Authored / requested PRs                      | Authored / assigned MRs                 |
| Discuss  | Reply, resolve/reopen when permitted          | Reply, resolve/reopen when permitted    |
| Submit   | Comment, approve, request changes             | Comment, approve                        |
| Merge    | Enabled merge/squash/rebase methods           | Project-default/squash policy           |
| CI       | Exact-SHA checks, statuses, workflow attempts | Exact-SHA MR pipelines and project jobs |

GitLab approval leaves composer text as a draft. GitLab URL subpaths/cross-project jobs, GitHub merge queues and auto-merge scheduling are unsupported. Missing capabilities fail closed; provider permissions and protections still apply.

## Write authority

[Native commands](mod.rs) derive the target from the owned review, then revalidate endpoints and provider revision. Renderer SHAs are not authority. Tokens stay in memory, bound to the exact host; authenticated redirects are disabled. Signed log downloads use separate unauthenticated requests.

The [ledger](ledger.rs) persists a pending receipt **before** sending. IDs bind to exact requests; matching actor/target/SHA/action fingerprints prevent duplicate sends across restarts. Receipts omit tokens and comment bodies. Pending/unknown outcomes cannot be evicted or automatically retried. **Reconcile** checks provider state; inconclusive results remain unknown. GitHub review submission pins `commit_id` but cannot atomically reject a concurrent push; merge SHA preconditions are provider-enforced.

## Resource policy

- Two shared network slots; 25-second HTTP deadlines and provider backoff.
- Coalesced, host/credential-scoped reads; 20-second cache TTL, 32 responses / 8 MiB total.
- Only visible focused details refresh, every 30 seconds. Pages and thread/log reads are bounded; incomplete checks never imply a green result.
- Connection changes cancel reads and clear caches. Panel/window close cancels reads; accepted writes survive their WebView and drain before quit.

See [validation](../../../VALIDATION.md) for checks and manual-service gaps.
