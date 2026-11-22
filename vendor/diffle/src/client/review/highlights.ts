/** CSS highlight styles are shared by a document; ranges and cleanup belong to a review.
 * Combining owners prevents a late unmount from clearing another review's highlights. */
const owners = new Map<string, Map<object, { ranges: Range[]; priority: number }>>();
let registry: unknown;
function available() {
  if (typeof CSS === 'undefined' || !('highlights' in CSS) || typeof Highlight === 'undefined') return false;
  if (registry !== CSS.highlights) {
    owners.clear();
    registry = CSS.highlights;
  }
  return true;
}
function render(name: string) {
  const values = owners.get(name);
  if (!values?.size) {
    owners.delete(name);
    CSS.highlights.delete(name);
    return;
  }
  const ranges = [...values.values()];
  const highlight = new Highlight(...ranges.flatMap((v) => v.ranges));
  highlight.priority = Math.max(...ranges.map((v) => v.priority));
  CSS.highlights.set(name, highlight);
}
export function paintHighlight(owner: object, name: string, ranges: Range[], priority = 0) {
  if (!available()) return;
  let values = owners.get(name);
  if (!values) {
    values = new Map();
    owners.set(name, values);
  }
  values.set(owner, { ranges, priority });
  render(name);
}
export function clearHighlight(owner: object, name: string) {
  if (!available()) return;
  owners.get(name)?.delete(owner);
  render(name);
}
