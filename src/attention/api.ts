import { invoke } from "@tauri-apps/api/core";
import type { ReviewRef, Site } from "../hosting/api";
export type Preferences = {
  revision: number;
  enabled: boolean;
  requested: boolean;
  checks: boolean;
  conflicts: boolean;
  agents: boolean;
  notifications: boolean;
  notifyRequested: boolean;
  notifyChecks: boolean;
  notifyConflicts: boolean;
  notifyAgents: boolean;
  tray: boolean;
  keepInTray: boolean;
  localTabs: string[];
  repositories: { site: Site; repository: string }[];
};
export type Target = {
  kind: string;
  workspace: string | null;
  tab: string | null;
  review: ReviewRef | null;
  head: string | null;
  url: string | null;
  root?: string | null;
  owner: string | null;
  context: string | null;
  terminal: string | null;
};
export type Item = {
  id: string;
  scope: string;
  source: string;
  title: string;
  detail: string;
  created: number;
  updated: number;
  resolved: number | null;
  read: boolean;
  dismissed: boolean;
  stale: boolean;
  paused: boolean;
  target: Target;
};
export type Repository = {
  workspace: string;
  workspaceName: string;
  tab: string;
  label: string;
  root: string;
  active: boolean;
};
export type View = {
  checking: boolean;
  lastChecked: number | null;
  revision: number;
  preferences: Preferences;
  items: Item[];
  running: Item[];
  repositories: Repository[];
  warnings: string[];
  error: string | null;
  trayError: string | null;
  unread: number;
  metrics: {
    cycles: number;
    localReads: number;
    remoteJobs: number;
    notifications: number;
  };
};
export type Delta = {
  revision: number;
  upsert: Item[];
  remove: string[];
  unread: number;
  running: Item[];
};
export function mergeDelta(view: View, delta: Delta): View {
  if (delta.revision <= view.revision) return view;
  const items = new Map(view.items.map((i) => [i.id, i]));
  for (const id of delta.remove) items.delete(id);
  for (const item of delta.upsert) items.set(item.id, item);
  return {
    ...view,
    revision: delta.revision,
    items: [...items.values()],
    unread: delta.unread,
    running: delta.running,
  };
}
export function mergeSnapshot(current: View | null, snapshot: View): View {
  if (!current) return snapshot;
  const next = snapshot.revision >= current.revision ? snapshot : current;
  return {
    ...next,
    preferences:
      snapshot.preferences.revision >= current.preferences.revision
        ? snapshot.preferences
        : current.preferences,
    repositories: snapshot.repositories,
  };
}
export const inbox = {
  view: () => invoke<View>("attention_view"),
  reload: () => invoke<View>("attention_reload"),
  save: (preferences: Preferences) =>
    invoke<View>("attention_preferences", { preferences }),
  update: (action: string, id: string | null = null) =>
    invoke<View>("attention_update", { action, id }),
  open: (id: string) => invoke<void>("attention_open", { id }),
};
