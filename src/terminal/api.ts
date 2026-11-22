import { Channel, invoke } from "@tauri-apps/api/core";
export type Exit = { code: number | null; message: string };
export type Info = {
  id: string;
  root: string;
  shell: string;
  pid: number;
  running: boolean;
  exit: Exit | null;
};
export type Preferences = {
  version: number;
  revision: number;
  enabled: boolean;
  fontSize: number;
  height: number;
};
export const terminalApi = {
  start: (
    context: string,
    worktree: string | null,
    onData: (bytes: ArrayBuffer) => void,
  ) =>
    invoke<Info>("terminal_start", {
      context,
      worktree,
      cols: 80,
      rows: 24,
      onEvent: new Channel<ArrayBuffer>(onData),
    }),
  agent: (preview: string, onData: (bytes: ArrayBuffer) => void) =>
    invoke<{ terminal: Info; checkout: string }>("agent_launch", {
      preview,
      externalTerminal: null,
      onEvent: new Channel<ArrayBuffer>(onData),
    }),
  list: () => invoke<Info[]>("terminal_list"),
  input: (id: string, data: Uint8Array) =>
    invoke<void>("terminal_input", { id, data: Array.from(data) }),
  ack: (id: string, seq: number) => invoke<void>("terminal_ack", { id, seq }),
  resize: (id: string, cols: number, rows: number) =>
    invoke<void>("terminal_resize", { id, cols, rows }),
  close: (id: string) => invoke<void>("terminal_close", { id }),
  closeOwner: () => invoke<void>("terminal_close_owner"),
  preferences: () =>
    invoke<Preferences>("terminal_preferences", {
      revision: null,
      preferences: null,
    }),
  save: (value: Preferences) =>
    invoke<Preferences>("terminal_preferences", {
      revision: value.revision,
      preferences: value,
    }),
};
