/** User-driven update flow. Constructing or subscribing never contacts the network. */
export type DownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };
export interface UpdatePackage {
  version: string;
  body?: string;
  download(
    progress: (event: DownloadEvent) => void,
    options: { timeout: number },
  ): Promise<void>;
  install(): Promise<void>;
  close(): Promise<void>;
}
export type UpdateInfo = {
  version: string;
  supported: boolean;
  reason: string | null;
};
export type UpdateState = {
  phase:
    | "idle"
    | "checking"
    | "current"
    | "available"
    | "downloading"
    | "installing"
    | "ready"
    | "restarting";
  version?: string;
  notes?: string;
  downloaded: number;
  total?: number;
  error?: string;
};
export class UpdateController {
  private state: UpdateState = { phase: "idle", downloaded: 0 };
  private update: UpdatePackage | null = null;
  private listeners = new Set<() => void>();
  constructor(privateServices: {
    info(): Promise<UpdateInfo>;
    check(): Promise<UpdatePackage | null>;
    restart(): Promise<void>;
  }) {
    this.services = privateServices;
  }
  private services: {
    info(): Promise<UpdateInfo>;
    check(): Promise<UpdatePackage | null>;
    restart(): Promise<void>;
  };
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private set(patch: Partial<UpdateState>) {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }
  private busy() {
    return [
      "checking",
      "downloading",
      "installing",
      "ready",
      "restarting",
    ].includes(this.state.phase);
  }
  async check() {
    if (this.busy()) return;
    this.set({
      phase: "checking",
      error: undefined,
      version: undefined,
      notes: undefined,
      downloaded: 0,
      total: undefined,
    });
    try {
      const info = await this.services.info();
      if (!info.supported)
        throw new Error(
          info.reason || "Updates are unavailable for this installation.",
        );
      const previous = this.update;
      this.update = null;
      await previous?.close();
      this.update = await this.services.check();
      this.set(
        this.update
          ? {
              phase: "available",
              version: this.update.version,
              notes: this.update.body,
            }
          : { phase: "current" },
      );
    } catch (error) {
      this.set({
        phase: "idle",
        error: `Could not check for updates. ${String(error).replace(/^Error: /, "")}`,
      });
    }
  }
  async install() {
    if (this.state.phase !== "available" || !this.update) return;
    this.set({
      phase: "downloading",
      error: undefined,
      downloaded: 0,
      total: undefined,
    });
    try {
      // The plugin verifies the signature before installation can proceed.
      await this.update.download(
        (event) => {
          if (event.event === "Started")
            this.set({ total: event.data.contentLength });
          if (event.event === "Progress")
            this.set({
              downloaded: this.state.downloaded + event.data.chunkLength,
            });
        },
        { timeout: 120_000 },
      );
      this.set({ phase: "installing" });
      await this.update.install();
      this.set({ phase: "ready" });
      await this.update.close().catch(() => {});
      this.update = null;
    } catch (error) {
      this.set({
        phase: "available",
        error: `Update failed. ${String(error).replace(/^Error: /, "")}`,
      });
    }
  }
  async restart() {
    if (this.state.phase !== "ready") return;
    this.set({ phase: "restarting", error: undefined });
    try {
      await this.services.restart();
    } catch (error) {
      this.set({
        phase: "ready",
        error: `Could not restart. ${String(error)}`,
      });
    }
  }
}
