/** Old WebView composers did not record a repository. Preserve them unchanged and
 * require an explicit assignment; a literal `working` key cannot identify its owner. */
export function legacyDrafts(commentKey: string): Record<string, string> {
  try {
    const source: unknown = JSON.parse(sessionStorage.getItem('diffle:unsent') ?? '{}');
    const assigned: Record<string, string> = JSON.parse(sessionStorage.getItem('diffle:unsent:assigned') ?? '{}');
    if (!source || typeof source !== 'object' || Array.isArray(source)) return {};
    return Object.fromEntries(
      Object.entries(source).filter(
        ([key, text]) =>
          key.startsWith(`${commentKey}\0`) && typeof text === 'string' && text.length > 0 && !assigned[key],
      ),
    );
  } catch {
    return {};
  }
}
export function assignLegacyDrafts(
  drafts: Record<string, string>,
  owner: string,
  save: (key: string, text: string) => void,
  current: (key: string) => string | undefined = () => undefined,
) {
  // Copy before recording assignment; a storage failure leaves the originals recoverable.
  const copied: string[] = [];
  for (const [key, text] of Object.entries(drafts)) {
    if (current(key)) continue;
    save(key, text);
    copied.push(key);
  }
  try {
    const assigned: Record<string, string> = JSON.parse(sessionStorage.getItem('diffle:unsent:assigned') ?? '{}');
    for (const key of copied) assigned[key] = owner;
    sessionStorage.setItem('diffle:unsent:assigned', JSON.stringify(assigned));
  } catch {
    /* Originals remain available until assignment can be recorded. */
  }
}
