/** Conservative accounting of retained data, not a measurement of JavaScript heap/RSS.
 * Parsed payloads are immutable; WeakMap entries do not keep evicted payloads alive. */
const sizes = new WeakMap<object, number>();
export function payloadBytes(value: unknown): number {
  if (typeof value === 'string') return value.length * 2;
  if (value == null || typeof value !== 'object') return 8;
  const cached = sizes.get(value);
  if (cached != null) return cached;
  // These payloads are JSON / parsed diff records. Never walk a DOM/tree-library instance.
  if (Object.getPrototypeOf(value) !== Object.prototype && !Array.isArray(value)) return 0;
  let bytes = 32;
  for (const [key, child] of Object.entries(value)) bytes += key.length * 2 + 8 + payloadBytes(child);
  sizes.set(value, bytes);
  return bytes;
}
export const REVIEW_PAYLOAD_BYTES = 64 * 1024 * 1024;
export const CLIENT_PAYLOAD_BYTES = 96 * 1024 * 1024;
