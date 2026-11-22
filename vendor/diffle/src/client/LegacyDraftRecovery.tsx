import { uiText } from '@/client/i18n/runtime.js';
import { useState } from 'react';
import { legacyDrafts, assignLegacyDrafts } from './legacyDrafts.js';
import { useReviewStore } from './store.js';

export function LegacyDraftRecovery() {
  const store = useReviewStore();
  const key = store((s) => s.snapshot?.commentKey);
  const [, refresh] = useState(0);
  const drafts = key ? legacyDrafts(key) : {};
  const count = Object.keys(drafts).length;
  if (!count) return null;
  return (
    <div className="legacy-drafts" role="status">
      <span>
        {count}
        {uiText(' earlier ')}
        {count === 1 ? uiText('draft has') : uiText('drafts have')}
        {uiText(' no recorded repository. Restore here only if this is their review.')}
      </span>
      <button
        onClick={() => {
          assignLegacyDrafts(drafts, key!, store.getState().setUnsent, (k) => store.getState().unsent[k]);
          refresh((v) => v + 1);
        }}
      >
        {uiText('Restore drafts here')}
      </button>
    </div>
  );
}
