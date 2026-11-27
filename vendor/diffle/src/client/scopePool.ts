import { z } from 'zod';
import { hunkProgressSchema, hunkVersion } from './review/hunks.js';
import { changedFilesByPath, itemIdOf, pathFromItemId } from './model.js';
const range = z.object({
  start: z.number().int().positive(),
  end: z.number().int().positive(),
  side: z.enum(['additions', 'deletions']).optional(),
  endSide: z.enum(['additions', 'deletions']).optional(),
});
const selection = z.object({ id: z.string().max(8192), range });
const graph = z.object({
  view: z.string().max(8192),
  state: z
    .object({
      selected: z.string().nullable(),
      selectedEdge: z.string().nullable(),
      filters: z.object({ symbols: z.boolean(), consumers: z.boolean(), tests: z.boolean() }),
      only: z.string().nullable(),
    })
    .optional(),
});
const position = z.object({
  path: z.string(),
  side: z.enum(['old', 'new']),
  line: z.number().int().positive(),
  full: z.boolean().optional(),
  external: z.boolean().optional(),
});
const jump = z.union([graph, position]);
const navigationSchema = z.object({
  activePath: z.string().nullable(),
  selection: selection.nullable(),
  collapsed: z.record(z.string(), z.boolean()),
  draft: z
    .object({
      path: z.string(),
      selection: selection.nullable(),
      hunk: z
        .object({
          version: z.string(),
          review: z.string(),
          side: z.enum(['old', 'new']),
          startLine: z.number().int().positive(),
          endLine: z.number().int().positive(),
        })
        .optional(),
      staleHunk: z.literal(true).optional(),
    })
    .nullable(),
  paneScroll: z.number().finite().nonnegative(),
  jumps: z.array(jump).max(200),
  jumpIndex: z.number().int().nonnegative(),
  view: z.enum(['diff', 'graph']),
  graph,
  viewport: z.object({ x: z.number().finite(), y: z.number().finite(), k: z.number().positive().finite() }).nullable(),
  snapshotKey: z.string().optional(),
  hunkProgress: hunkProgressSchema.default([]).catch([]),
  versions: z.record(z.string(), z.string()).default({}),
  fileView: z
    .object({ path: z.string(), from: z.object({ position: position.nullable(), activePath: z.string().nullable() }) })
    .nullable()
    .default(null),
});
export function parseReviewNavigation(value: unknown) {
  const parsed = navigationSchema.safeParse(value);
  return parsed.success ? parsed.data : null;
}
import { createReviewScope, type ReviewScope } from './runtime.js';
import type { ReviewTransport } from './transport.js';
import { CLIENT_PAYLOAD_BYTES, payloadBytes } from './payloadBudget.js';
import type { StoreIdentity } from './store.js';

/** One visible subtree and at most one warm payload per WebView. Cold entries retain
 * small navigation descriptors; composers persist under their independent owner key. */
export class ReviewScopePool {
  private live = new Map<string, { scope: ReviewScope; stop: () => void }>();
  private cold = new Map<string, ReturnType<ReviewScopePool['navigation']>>();
  private active: string | null = null;
  constructor(
    private readonly byteLimit = CLIENT_PAYLOAD_BYTES,
    private readonly liveLimit = 2,
  ) {}
  navigation(scope: ReviewScope) {
    const s = scope.store.getState();
    const collapsed: Record<string, boolean> = {};
    let cost = 0;
    for (const path in s.collapsed) {
      cost += path.length * 2 + 32;
      if (cost > 24 * 1024) break;
      collapsed[path] = s.collapsed[path]!;
    }
    const files = s.snapshot ? changedFilesByPath(s.snapshot.changed) : new Map();
    const paths = new Set(
      [
        ...Object.keys(collapsed),
        s.activePath,
        s.draft?.path,
        s.selection ? pathFromItemId(s.selection.id) : null,
      ].filter((v): v is string => !!v),
    );
    const versions: Record<string, string> = {};
    for (const path of paths) {
      const f = files.get(path);
      versions[path] = JSON.stringify([f?.oldBlob, f?.blob]);
    }
    const value = {
      snapshotKey: s.snapshot
        ? JSON.stringify([s.snapshot.oldSha, s.snapshot.newSha, s.snapshot.commentKey])
        : undefined,
      hunkProgress: s.hunkProgress,
      activePath: s.activePath,
      selection: s.selection,
      collapsed,
      versions,
      fileView: s.fileView && !s.fileView.external ? { path: s.fileView.path, from: s.fileView.from } : null,
      draft: s.draft,
      paneScroll: s.paneScroll,
      jumps: s.jumps.filter((place) => !('restore' in place)),
      jumpIndex: s.jumpIndex,
      view: scope.views.store.getState().active,
      graph: scope.graph.place(),
      viewport: scope.graph.store.getState().viewport,
    };
    if (new TextEncoder().encode(JSON.stringify({ ...value, hunkProgress: [] })).length > 64 * 1024) {
      value.collapsed = {};
      value.versions = Object.fromEntries(
        [...paths]
          .filter(
            (p) =>
              p === s.activePath || p === s.draft?.path || p === (s.selection ? pathFromItemId(s.selection.id) : null),
          )
          .map((p) => [p, versions[p]!]),
      );
      value.jumps = [];
      value.jumpIndex = 0;
      value.graph = {
        view: 'graph:',
        state: { selected: null, selectedEdge: null, filters: { ...scope.graph.store.getState().filters }, only: null },
      };
    }
    return value;
  }
  acquire(
    id: string,
    transport: () => ReviewTransport,
    identity: StoreIdentity,
    initialNavigation?: unknown,
  ): ReviewScope {
    this.active = id;
    const cached = this.live.get(id);
    if (cached) {
      this.live.delete(id);
      this.live.set(id, cached);
      return cached.scope;
    }
    while (this.live.size >= this.liveLimit) this.evict(this.live.keys().next().value!);
    const scope = createReviewScope(transport(), identity);
    const owner = JSON.stringify(identity);
    const saved = parseReviewNavigation(initialNavigation) ?? this.cold.get(owner);
    if (saved) {
      let cancelled = false;
      const restore = scope.store.subscribe((state, previous) => {
        if (!state.snapshot || state.snapshot === previous.snapshot) return;
        restore();
        const snapshot = state.snapshot;
        // applySnapshot resets mode-local navigation after notifying subscribers.
        queueMicrotask(() => {
          void (async () => {
            if (cancelled || scope.store.getState().snapshot !== snapshot) return;
            const same = saved.snapshotKey === JSON.stringify([snapshot.oldSha, snapshot.newSha, snapshot.commentKey]);
            const mutable = snapshot.oldSha === 'worktree' || snapshot.newSha === 'worktree';
            const sameFile = (path: string) => {
              if (!same) return false;
              if (!mutable) return true;
              const f = changedFilesByPath(snapshot.changed).get(path);
              return saved.versions?.[path] === JSON.stringify([f?.oldBlob, f?.blob]);
            };
            if (saved.fileView && snapshot.tree.includes(saved.fileView.path))
              await scope.store.getState().openFullFile(saved.fileView.path);
            const state = scope.store.getState();
            if (cancelled || state.snapshot !== snapshot) return;
            const normalize = (selection: NonNullable<typeof saved.selection>) => ({
              ...selection,
              id: itemIdOf(state, pathFromItemId(selection.id)),
            });
            const selection =
              saved.selection && sameFile(pathFromItemId(saved.selection.id)) ? normalize(saved.selection) : null;
            const collapsed = Object.fromEntries(Object.entries(saved.collapsed).filter(([path]) => sameFile(path)));
            const draft = saved.draft;
            const hunk = draft?.hunk;
            const file = draft && changedFilesByPath(snapshot.changed).get(draft.path);
            const restoreDraft =
              draft &&
              ((hunk && hunk.review === snapshot.commentKey) ||
                (snapshot.tree.includes(draft.path) && (!draft.selection || sameFile(draft.path))));
            scope.store.setState({
              hunkProgress: saved.hunkProgress,
              activePath: saved.activePath && snapshot.tree.includes(saved.activePath) ? saved.activePath : null,
              selection,
              collapsed,
              draft: restoreDraft
                ? {
                    ...draft,
                    selection: draft.selection ? normalize(draft.selection) : null,
                    staleHunk:
                      draft.staleHunk || (hunk && (!file || hunk.version !== hunkVersion(snapshot, file))) || undefined,
                  }
                : null,
              paneScroll: same && (!saved.activePath || sameFile(saved.activePath)) ? saved.paneScroll : 0,
              jumps: same ? saved.jumps : [],
              jumpIndex: same ? saved.jumpIndex : 0,
              ...(state.fileView && saved.fileView
                ? { fileView: { ...state.fileView, from: saved.fileView.from } }
                : {}),
            });
            if (saved.view === 'graph' && same) {
              scope.graph.restore(saved.graph);
              scope.graph.store.setState({ viewport: saved.viewport });
            }
          })().catch(() => {});
        });
      });
      scope.onDispose(() => {
        cancelled = true;
        restore();
      });
    }
    let enforcing = false;
    const enforce = () => {
      if (enforcing || this.bytes() <= this.byteLimit) return;
      enforcing = true;
      try {
        for (const key of this.live.keys()) {
          if (key !== this.active) this.evict(key);
          if (this.bytes() <= this.byteLimit) break;
        }
        if (this.bytes() > this.byteLimit) {
          this.cold.set(owner, this.navigation(scope));
          while (this.cold.size > 64) this.cold.delete(this.cold.keys().next().value!);
          scope.graph.dispose();
          scope.views.reset();
          scope.store.getState().endReview();
          scope.store.setState({ error: 'Review payload budget reached. Open a narrower comparison.' });
        }
      } finally {
        enforcing = false;
      }
    };
    const reviewStop = scope.store.subscribe((state, previous) => {
      if (
        state.loaded !== previous.loaded ||
        state.contents !== previous.contents ||
        state.fileView !== previous.fileView ||
        state.snapshot !== previous.snapshot
      )
        enforce();
    });
    const graphStop = scope.graph.store.subscribe(() => enforce());
    this.live.set(id, {
      scope,
      stop: () => {
        reviewStop();
        graphStop();
      },
    });
    return scope;
  }
  seed(identity: StoreIdentity, value: unknown) {
    const parsed = parseReviewNavigation(value);
    if (parsed) {
      this.cold.set(JSON.stringify(identity), { ...parsed, snapshotKey: parsed.snapshotKey });
      while (this.cold.size > 64) this.cold.delete(this.cold.keys().next().value!);
    }
  }
  peek(id: string) {
    return this.live.get(id)?.scope;
  }
  capture(id: string) {
    const scope = this.peek(id);
    return scope ? { unsent: scope.store.getState().unsent, navigation: this.navigation(scope) } : null;
  }
  private evict(id: string) {
    const item = this.live.get(id);
    if (!item) return;
    this.cold.set(JSON.stringify(item.scope.identity), this.navigation(item.scope));
    while (this.cold.size > 64) this.cold.delete(this.cold.keys().next().value!);
    this.live.delete(id);
    item.stop();
    item.scope.dispose();
  }
  close(id: string) {
    this.evict(id);
    if (this.active === id) this.active = null;
  }
  bytes() {
    let bytes = 0;
    for (const { scope } of this.live.values()) {
      const s = scope.store.getState();
      bytes +=
        payloadBytes(s.snapshot) +
        payloadBytes(s.loaded) +
        payloadBytes(s.contents) +
        payloadBytes(s.fileView?.item) +
        payloadBytes(scope.graph.store.getState().view);
    }
    return bytes;
  }
  diagnostics() {
    return { live: this.live.size, cold: this.cold.size, payloadBytes: this.bytes() };
  }
  dispose() {
    for (const id of this.live.keys()) this.evict(id);
    this.cold.clear();
    this.active = null;
  }
}
