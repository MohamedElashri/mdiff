# Agents and review

Configure detection, executable paths, and hooks in **Preferences → Agents**. Detection is off by default and follows the selected repository without opening its panel. Open **Tools → Agents and review → Activity** to scan and inspect coverage. Tool checks never run agents; unsaved preferences require save/discard before closing.

The current list excludes exited processes; recent hook/launch reports expire separately. Process identities survive review-runtime changes, and hook reports update the same session row.

## Review handoff

Select **Review commit with agent** in History or prepare an open comparison. **Prepare agent review** shows immutable endpoints, tool, isolated folder and exact prompt; launch is a separate confirmation. Direct launch requires two commit endpoints. Working/empty-tree comparisons support **Copy comment prompt**.

[Native launch](../../src-tauri/src/agents/launch.rs) consumes a window-bound, single-use preview and rechecks endpoints/preferences. It creates an independent repository with shallow-fetched endpoints and detached HEAD, leaving the source untouched. Submodules, LFS, intermediate history and project setup are not materialized. At most 16 isolated folders are retained under `agent-reviews-v1/`; inspect/remove them manually. Restart never resumes agents.

Embedded launches use [terminal ownership](../terminal/README.md). Supported command-capable external terminals receive literal argv; mdiff does not own their processes. Codex uses read-only/on-request settings; Claude retains its CLI permissions. No bypass flags or app connection credentials are supplied. The review prompt alone is not a sandbox guarantee.

## Activity and hooks

One opt-in [monitor](../../src-tauri/src/agents/mod.rs) observes executable/UID/parent/start/cwd metadata for known checkouts, never argv, environments or transcripts. Sampling is bounded; disabled/no-context monitoring has no timer. Process detection means **unknown task state**; only explicit launches establish pinned provenance and owned-terminal actions. Changed start/cwd invalidates attribution.

**Generate hook bundle** writes mdiff helpers/config, without editing user settings. **Accept local state hooks** adds metadata hooks to new isolated launches while preserving existing policies. Helpers require Python 3. The listener validates kernel peer identity, agent ancestry, start/cwd/session and configuration generation; reports expire to unknown. Hooks transmit bounded metadata only, not prompts/output or permission decisions. Same-account malicious code is outside this boundary.

Uninstall makes the helper inert so old references do not block tools; remove manually merged references separately. Disabling hooks/detection never stops unrelated agents.

## Findings

Paste `{"findings":[{"path":"file.ts","start":2,"end":3,"body":"Issue"}]}`. Validation requires changed regular text paths and valid head-side lines, capped at 128 KiB/100 findings. Draft JSON persists with the tab; restored/changed comparisons require revalidation. Tool attribution is user-selected. Nothing publishes or edits automatically. See [validation](../../VALIDATION.md).
