import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { configureTransport } from "../vendor/diffle/src/client/transport";
export function bindReview(id: string) {
  configureTransport({
    async request(path, init) {
      init?.signal?.throwIfAborted();
      const packet = await invoke<ArrayBuffer>("review_request", {
        id,
        path: "/" + path.replace(/^\//, ""),
        method: init?.method ?? "GET",
        body: init?.body ?? null,
      });
      init?.signal?.throwIfAborted();
      const length = new DataView(packet).getUint32(0, true);
      if (length > packet.byteLength - 4)
        throw new Error("Invalid native response");
      const header = JSON.parse(
        new TextDecoder().decode(new Uint8Array(packet, 4, length)),
      ) as { status: number; contentType: string };
      return new Response(
        header.status === 204 ? null : new Uint8Array(packet, 4 + length),
        {
          status: header.status,
          headers: { "content-type": header.contentType },
        },
      );
    },
    subscribe(message, ready, lost) {
      let closed = false;
      const subscription = listen<{ id: string; message: string }>(
        "review-update",
        (e) => {
          if (closed || e.payload.id !== id) return;
          if (e.payload.message === "resync") ready();
          else message(JSON.parse(e.payload.message));
        },
      );
      void subscription
        .then((stop) => {
          if (closed) stop();
          else ready();
        })
        .catch(lost);
      return () => {
        closed = true;
        void subscription.then((stop) => stop());
      };
    },
  });
}
