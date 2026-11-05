/** A partial patch is renderable only up to the start of its last hunk.
 * The final response includes that hunk and any no-newline markers verbatim. */
export function completePatchPrefix(patch: string, done: boolean): string {
  if (done) return patch;
  let last = 0;
  let count = 0;
  for (const hunk of patch.matchAll(/^@@ -\d+(?:,\d+)? \+\d+(?:,\d+)? @@/gm)) {
    last = hunk.index;
    count++;
  }
  return count > 1 ? patch.slice(0, last) : '';
}
