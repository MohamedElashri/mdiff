import type { ReviewState } from '@/client/store.js';
import { api as defaultApi, type ReviewApi } from '@/client/api.js';
import { messages, translate } from '@/client/i18n/messages.js';
import { rankMatches } from './search.js';
import type { PaletteEntry, SearchSource } from './registry.js';

export function reviewSources(
  getState: () => ReviewState,
  onNavigate: () => void = () => {},
  api: ReviewApi = defaultApi,
): SearchSource[] {
  const guard = (snapshot: ReviewState['snapshot'], run: () => unknown) => () => {
    if (!snapshot || getState().snapshot !== snapshot) return false;
    return run();
  };
  return [
    {
      id: 'review.files',
      group: 'files',
      open() {
        const snapshot = getState().snapshot;
        const paths = snapshot ? [...new Set([...snapshot.tree, ...snapshot.changed.map((file) => file.path)])] : [];
        return {
          search(query) {
            if (!snapshot || getState().snapshot !== snapshot) return [];
            return rankMatches(paths, query, (path) => path).map((path): PaletteEntry => ({
              id: `file:${path}`,
              group: 'files',
              label: path,
              detail: translate(messages.openFile),
              run: guard(snapshot, async () => {
                await getState().openFile(path);
                if (getState().snapshot === snapshot) onNavigate();
              }),
            }));
          },
        };
      },
    },
    {
      id: 'review.commits',
      group: 'commits',
      open() {
        const snapshot = getState().snapshot;
        return {
          search(query) {
            if (!snapshot || getState().snapshot !== snapshot) return [];
            return rankMatches(
              snapshot.commits.list,
              query,
              (commit) => `${commit.short} ${commit.message.split('\n', 1)[0]} ${commit.author}`,
            ).map((commit): PaletteEntry => ({
              id: `commit:${commit.sha}`,
              group: 'commits',
              label: commit.message.split('\n', 1)[0]!.slice(0, 200),
              detail: translate(messages.comparisonCommit, {
                sha: commit.short,
                author: commit.author,
              }),
              run: guard(snapshot, () => {
                getState().focusCommit(commit.sha);
                onNavigate();
              }),
            }));
          },
        };
      },
    },
    {
      id: 'review.refs',
      group: 'refs',
      open() {
        const snapshot = getState().snapshot;
        const abort = new AbortController();
        let loading: Promise<{ name: string; ref: string; kind: string }[]> | undefined;
        return {
          dispose: () => abort.abort(),
          async search(query, signal) {
            if (!snapshot || getState().snapshot !== snapshot || signal.aborted) return [];
            // One request per palette session, shared across successive queries; close cancels it.
            loading ??= api.refs(abort.signal).then((refs) => [
              ...refs.branches.map((name) => ({
                name,
                ref: `refs/heads/${name}`,
                kind: translate(messages.branch),
              })),
              ...refs.remoteBranches.map((name) => ({
                name,
                ref: `refs/remotes/${name}`,
                kind: translate(messages.remoteBranch),
              })),
              ...refs.tags.map((name) => ({
                name,
                ref: `refs/tags/${name}`,
                kind: translate(messages.tag),
              })),
            ]);
            const entries = await loading;
            if (signal.aborted || abort.signal.aborted || getState().snapshot !== snapshot) return [];
            return rankMatches(entries, query, (entry) => `${entry.name} ${entry.kind}`).map((entry): PaletteEntry => ({
              id: `ref:${entry.ref}`,
              group: 'refs',
              label: entry.name,
              detail: translate(messages.compareRef, { kind: entry.kind }),
              run: guard(snapshot, () => {
                const result = getState().enter({ kind: 'revspec', args: [entry.ref] });
                onNavigate();
                return result;
              }),
            }));
          },
        };
      },
    },
  ];
}
