import type { ServerMessage } from '@/shared/protocol.js';
export interface ReviewTransport {
  request(path: string, init?: RequestInit): Promise<Response>;
  subscribe(message: (msg: ServerMessage) => void, ready: () => void, lost: () => void): () => void;
}
let transport: ReviewTransport | undefined;
export function configureTransport(value: ReviewTransport) {
  transport = value;
}
export function reviewTransport() {
  return transport;
}
export function request(path: string, init?: RequestInit) {
  return transport ? transport.request(path, init) : fetch(new URL(path, location.href).href, init);
}
