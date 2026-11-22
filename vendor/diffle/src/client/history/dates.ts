/** Match the local calendar dates shown in rows, including 23/25-hour DST days. */
export function historyDate(value: string, end = false): number | null {
  if (!value) return null;
  const date = new Date(`${value}T00:00:00`);
  if (end) date.setDate(date.getDate() + 1);
  return Math.floor(date.getTime() / 1000) - (end ? 1 : 0);
}
