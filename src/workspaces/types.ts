import type { EnterRequest } from "../../vendor/diffle/src/shared/protocol";
export type Request = EnterRequest | { kind: "gitlab"; url: string };
export interface Source {
  path: string;
  request: Request;
  label: string;
}
export type Policy = "follow" | "pinned";
export interface Tab {
  id: string;
  source: Source;
  group: string | null;
  policy: Policy;
  drafts: number;
  saved: null | {
    key: string;
    root: string;
    archive: string | null;
    old: string;
    new: string;
    head: string;
    review: { kind: string; old?: string; new?: string };
  };
}
export interface Group {
  id: string;
  name: string;
  color: string;
  collapsed: boolean;
}
export interface Workspace {
  id: string;
  name: string;
  tabs: Tab[];
  groups: Group[];
  active: string | null;
  recent: Source[];
}
export interface Document {
  version: 1;
  revision: number;
  activeWorkspace: string;
  workspaces: Workspace[];
  closed: { workspace: string; tab: Tab }[];
}
export interface View {
  revision: number;
  unsent: Record<string, string>;
  navigation: unknown | null;
}
export interface Opened {
  id: string;
  root: string;
  key: string;
  github_enabled: boolean;
}
export interface Activated {
  opened: Opened;
  tab: Tab;
  view: View;
  notice: string | null;
  gitlab: boolean;
}
export type Operation =
  | { kind: "createWorkspace"; name: string }
  | { kind: "selectWorkspace"; id: string }
  | { kind: "renameWorkspace"; id: string; name: string }
  | { kind: "deleteWorkspace"; id: string }
  | { kind: "addTab"; workspace: string; source: Source }
  | { kind: "closeTab"; id: string }
  | { kind: "reopenTab" }
  | { kind: "moveWorkspace"; id: string; workspace: string }
  | { kind: "moveTab"; id: string; before: string | null }
  | { kind: "renameTab"; id: string; label: string }
  | { kind: "setGroup"; id: string; group: string | null }
  | { kind: "setPolicy"; id: string; policy: Policy }
  | { kind: "createGroup"; workspace: string; name: string; color: string }
  | {
      kind: "editGroup";
      id: string;
      name?: string;
      color?: string;
      collapsed?: boolean;
    }
  | { kind: "deleteGroup"; id: string };

export interface WorkspaceWindow {
  label: string;
  workspace: string | null;
  current: boolean;
}
export interface ConnectionProfiles {
  token: string | null;
  tokenHost: string | null;
  gitlabToken: string | null;
  gitlabHost: string | null;
}
