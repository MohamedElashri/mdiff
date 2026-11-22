import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  configureTransport,
  type ReviewResponse,
  type ReviewTransport,
} from "../vendor/diffle/src/client/transport";

const decoder = new TextDecoder();

export function decodeResponse(packet: ArrayBuffer): ReviewResponse {
  if (packet.byteLength < 4) throw new Error("Invalid native response");
  const length = new DataView(packet).getUint32(0, true);
  if (length > packet.byteLength - 4)
    throw new Error("Invalid native response");

  const { status, contentType } = JSON.parse(
    decoder.decode(new Uint8Array(packet, 4, length)),
  ) as { status: number; contentType: string };
  // A Response constructor copies its input. Decode directly from the IPC packet.
  const body = new Uint8Array(packet, 4 + length);
  return {
    status,
    ok: status >= 200 && status < 300,
    statusText: "",
    text: async () => decoder.decode(body),
    json: async () => JSON.parse(decoder.decode(body)),
    blob: async () => new Blob([body], { type: contentType }),
  };
}

export function nativeReviewTransport(id: string): ReviewTransport {
  return {
    async request(path, init) {
      const signal = init?.signal;
      signal?.throwIfAborted();
      const requestId = signal ? crypto.randomUUID() : undefined;
      const cancel = () => {
        void invoke("cancel_review_request", { id, requestId }).catch(() => {});
      };
      signal?.addEventListener("abort", cancel, { once: true });
      let packet: ArrayBuffer;
      try {
        packet = await invoke<ArrayBuffer>("review_request", {
          requestId,
          id,
          path: "/" + path.replace(/^\//, ""),
          method: init?.method ?? "GET",
          body: init?.body ?? null,
        });
      } catch (error) {
        signal?.throwIfAborted();
        throw error;
      } finally {
        signal?.removeEventListener("abort", cancel);
      }
      signal?.throwIfAborted();
      return decodeResponse(packet);
    },
    subscribe(message, ready, lost) {
      let closed = false;
      let unlisten: UnlistenFn | undefined;
      void listen<{ id: string; message: string }>("review-update", (e) => {
        if (closed || e.payload.id !== id) return;
        if (e.payload.message === "resync") ready();
        else message(JSON.parse(e.payload.message));
      })
        .then((stop) => {
          if (closed) return stop();
          unlisten = stop;
          ready();
        })
        .catch(() => {
          if (!closed) lost();
        });
      return () => {
        closed = true;
        void Promise.resolve(unlisten?.()).catch(() => {});
        unlisten = undefined;
      };
    },
  };
}

export function bindReview(id: string) {
  configureTransport(nativeReviewTransport(id));
}
