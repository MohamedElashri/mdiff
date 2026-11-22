import { message } from '@/client/i18n/runtime.js';
export const historyMessages = {
  title: message('history.title', 'History'),
  changes: message('history.changes', 'Changes'),
  refresh: message('history.refresh', 'Refresh refs'),
  search: message('history.search', 'Subject or SHA'),
  author: message('history.author', 'Author'),
  apply: message('history.apply', 'Apply filters'),
  info: message('history.pinned', 'History is pinned to the selected refs. Refresh to include external changes.'),
  changed: message('history.changed', 'Refs changed outside mdiff. Refresh history when ready.'),
  filtered: message(
    'history.filtered',
    'Ancestry links are hidden while filtering or folding; parent IDs remain available in details.',
  ),
  empty: message('history.empty', 'No commits in this page. Continue searching or choose another root.'),
};
