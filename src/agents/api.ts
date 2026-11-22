import { Channel, invoke } from "@tauri-apps/api/core";
export type Activity = {
  id: string;
  tool: string;
  session: string | null;
  parent: string | null;
  root: string;
  context: string;
  state: string;
  provenance: string;
  started: number;
  seen: number;
  pid: number;
  start: string;
  commit: string | null;
  terminal: string | null;
};
export type Preferences = {
  revision: number;
  enabled: boolean;
  hooks: boolean;
  codex: string | null;
  claude: string | null;
  focus: string;
};
export type Tool = {
  id: string;
  path: string | null;
  source: "configured" | "automatic";
  error: string | null;
};
export type Scan = {
  running: boolean;
  completed: number | null;
  checked: number;
  total: number;
  readable: number;
  limited: boolean;
  error: string | null;
};
export type View = {
  version: number;
  scan: Scan;
  roots: string[];
  hookError: string | null;
  hookReady: boolean;
  lastHook: number | null;
  preferences: Preferences;
  activities: Activity[];
  tools: Tool[];
  terminals: { id: string; name: string }[];
  error: string | null;
  hooks: {
    helper: string;
    claude: string;
    codex: string;
    socket: string;
  } | null;
};
export type Preview = {
  executable: string;
  revision: number;
  id: string;
  root: string;
  checkout: string;
  base: string;
  head: string;
  tool: string;
  profile: string;
  prompt: string;
  created: number;
};
export type Finding = {
  path: string;
  start: number;
  end: number;
  body: string;
};
export const agentApi = {
  view: (context: string | null, observer: string) =>
    invoke<View>("agent_view", { context, observer }),
  settings: () => invoke<View>("agent_settings"),
  scan: () => invoke<View>("agent_scan"),
  reload: () => invoke<View>("agent_reload"),
  tools: (preferences: Preferences) =>
    invoke<Tool[]>("agent_tools", { preferences }),
  save: (preferences: Preferences) =>
    invoke<View>("agent_preferences", {
      revision: preferences.revision,
      preferences,
    }),
  external: (preview: string, externalTerminal: string) =>
    invoke<{ checkout: string }>("agent_launch", {
      preview,
      externalTerminal,
      onEvent: new Channel<ArrayBuffer>(() => {}),
    }),
  unwatch: (context: string | null, observer: string) =>
    invoke("agent_unwatch", { context, observer }),
  hookConfig: (tool: string) => invoke<string>("agent_hook_config", { tool }),
  hooks: (install: boolean) => invoke<View>("agent_hooks", { install }),
  preview: (context: string, tool: string, profile: string) =>
    invoke<Preview>("agent_preview", { context, tool, profile }),
  import: (context: string, document: string, tool: string) =>
    invoke<{ base: string; head: string; tool: string; findings: Finding[] }>(
      "agent_import",
      { context, document, tool },
    ),
};
