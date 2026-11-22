// Native IPC fixture for desktop shell tests. Rust suites validate persistence and Git ownership.
// oxlint-disable-next-line no-restricted-imports -- Desktop IPC contract.
import type { Activated, Document, Source, View, Operation } from '../../../../src/workspaces/types';
export function workspaceFixture() {
  let n = 0;
  const document: Document = {
    version: 1,
    revision: 0,
    activeWorkspace: 'a'.repeat(64),
    workspaces: [
      {
        id: 'a'.repeat(64),
        name: 'Default',
        tabs: [],
        groups: [],
        active: null,
        recent: [],
      },
    ],
    closed: [],
  };
  const views = new Map<string, View>();
  let activateHook: ((tab: string) => Promise<Activated>) | null = null;
  const fixture = {
    document,
    views,
    setActivate(hook: typeof activateHook) {
      activateHook = hook;
    },
    async invoke(command: string, args: Record<string, unknown> = {}): Promise<unknown> {
      if (command === 'terminal_list') return [];
      if (command === 'shell_preferences') return args.legacy;
      if (command === 'save_shell_preferences')
        return {
          ...(args.preferences as object),
          revision: Number(args.revision) + 1,
        };
      if (command === 'desktop_settings') return { githubEnabled: false, lspCommands: {}, githubHosts: {} };
      if (command === 'workspace_load' || command === 'workspace_capture') return structuredClone(document);
      if (command === 'workspace_update') {
        if (args.revision !== document.revision) throw Error('Workspace changed; reload and retry');
        const op = args.operation as Operation;
        const w = document.workspaces.find((w) => w.id === document.activeWorkspace)!;
        if (op.kind === 'addTab') {
          let tab = w.tabs.find(
            (t) =>
              t.source.path === op.source.path &&
              JSON.stringify(t.source.request) === JSON.stringify(op.source.request),
          );
          if (!tab) {
            tab = {
              id: String(++n).padStart(64, '0'),
              source: op.source,
              group: null,
              policy: 'follow',
              drafts: 0,
              saved: null,
            };
            w.tabs.push(tab);
          }
          w.active = tab.id;
        } else if (op.kind === 'createWorkspace') {
          const id = String(++n).padStart(64, 'f');
          document.workspaces.push({
            id,
            name: op.name,
            tabs: [],
            groups: [],
            recent: [],
            active: null,
          });
          document.activeWorkspace = id;
        } else if (op.kind === 'selectWorkspace') {
          document.activeWorkspace = op.id;
        } else if (op.kind === 'moveWorkspace') {
          const from = document.workspaces.find((w) => w.tabs.some((t) => t.id === op.id))!;
          const i = from.tabs.findIndex((t) => t.id === op.id);
          const tab = from.tabs.splice(i, 1)[0]!;
          tab.group = null;
          document.workspaces.find((w) => w.id === op.workspace)!.tabs.push(tab);
          from.active = from.tabs[0]?.id ?? null;
        } else if (op.kind === 'closeTab') {
          const i = w.tabs.findIndex((t) => t.id === op.id);
          if (i >= 0) {
            document.closed.unshift({
              workspace: w.id,
              tab: w.tabs.splice(i, 1)[0]!,
            });
            w.active = w.tabs[0]?.id ?? null;
          }
        } else if (op.kind === 'reopenTab') {
          const c = document.closed.shift();
          if (c) {
            const target = document.workspaces.find((w) => w.id === c.workspace)!;
            target.tabs.push(c.tab);
            target.active = c.tab.id;
            document.activeWorkspace = c.workspace;
          }
        }
        document.revision++;
        return structuredClone(document);
      }
      if (command === 'workspace_activate') {
        const id = String(args.tab);
        if (activateHook) return activateHook(id);
        return fixture.activated(id);
      }
      if (command === 'workspace_read_view')
        return structuredClone(
          views.get(String(args.tab)) ?? {
            revision: 0,
            unsent: {},
            navigation: null,
          },
        );
      if (command === 'workspace_save_view') {
        const view = args.view as View;
        const old = views.get(String(args.tab));
        if ((old?.revision ?? 0) !== args.revision) throw Error('Tab state changed');
        const next = { ...view, revision: Number(args.revision) + 1 };
        views.set(String(args.tab), next);
        return structuredClone(next);
      }
      return undefined;
    },
    activated(id: string): Activated {
      const w = document.workspaces.find((w) => w.tabs.some((t) => t.id === id))!;
      const tab = w.tabs.find((t) => t.id === id)!;
      w.active = id;
      w.recent = [tab.source, ...w.recent.filter((s) => s.path !== tab.source.path)];
      document.revision++;
      return {
        opened: {
          id,
          root: tab.source.path,
          key: `workspace/${id}/local:${tab.source.path}`,
          github_enabled: false,
        },
        tab: structuredClone(tab),
        view: structuredClone(views.get(id) ?? { revision: 0, unsent: {}, navigation: null }),
        notice: null,
        gitlab: false,
      };
    },
    add(source: Source) {
      const tab = {
        id: String(++n).padStart(64, '0'),
        source,
        group: null,
        policy: 'follow' as const,
        drafts: 0,
        saved: null,
      };
      document.workspaces[0]!.tabs.push(tab);
      return tab;
    },
  };
  return fixture;
}
