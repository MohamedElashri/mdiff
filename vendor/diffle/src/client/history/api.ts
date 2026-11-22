import { z } from 'zod';
import { request, type ReviewResponse } from '@/client/transport.js';
import {
  HistoryInfoSchema,
  HistoryPageSchema,
  HistoryDetailSchema,
  HistoryTreeSchema,
  EnterRequestSchema,
  type HistoryQuery,
} from '@/shared/protocol.js';
export type HistoryTransport = (path: string, init?: RequestInit) => Promise<ReviewResponse>;
export function historyApi(transport: HistoryTransport = request) {
  async function json<T>(schema: z.ZodType<T>, path: string, init?: RequestInit): Promise<T> {
    const response = await transport(`api/history/${path}`, init);
    const value = await response.json();
    if (!response.ok) throw new Error(typeof value?.error === 'string' ? value.error : 'Repository read failed');
    return schema.parse(value);
  }
  const body = (data: unknown, signal?: AbortSignal): RequestInit => ({
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(data),
    signal,
  });
  return {
    info: (signal?: AbortSignal) => json(HistoryInfoSchema, 'info', { signal }),
    begin: (query: HistoryQuery, signal: AbortSignal, generation: string) =>
      json(HistoryPageSchema, 'query', body({ ...query, generation }, signal)),
    page: (generation: string, page: number, signal: AbortSignal) =>
      json(HistoryPageSchema, `page/${encodeURIComponent(generation)}/${page}`, { signal }),
    detail: (sha: string, signal: AbortSignal) => json(HistoryDetailSchema, `commit/${sha}`, { signal }),
    tree: (sha: string, signal: AbortSignal) => json(HistoryTreeSchema, `tree/${sha}`, { signal }),
    compare: (kind: string, commits: string[], parent: string | null, signal: AbortSignal) =>
      json(EnterRequestSchema, 'compare', body({ kind, commits, parent }, signal)),
    close: (generation: string) => json(z.boolean(), `query/${generation}`, { method: 'DELETE' }),
  };
}
