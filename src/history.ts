import { invoke } from "@tauri-apps/api/core";
import { decodeResponse } from "./transport";
import type { HistoryTransport } from "../vendor/diffle/src/client/history/api";
export function repositoryTransport(id: string): HistoryTransport {
  return async (path, init) => {
    const requestId = crypto.randomUUID();
    const signal = init?.signal;
    signal?.throwIfAborted();
    const cancel = () => {
      void invoke("cancel_review_request", { id, requestId }).catch(() => {});
    };
    signal?.addEventListener("abort", cancel, { once: true });
    try {
      const packet = await invoke<ArrayBuffer>("repository_request", {
        id,
        requestId,
        path: "/" + path.replace(/^\//, ""),
        method: init?.method ?? "GET",
        body: init?.body ?? null,
      });
      signal?.throwIfAborted();
      return decodeResponse(packet);
    } catch (error) {
      signal?.throwIfAborted();
      throw error;
    } finally {
      signal?.removeEventListener("abort", cancel);
    }
  };
}
