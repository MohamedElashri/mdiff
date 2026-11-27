import { uiText } from '@/client/i18n/runtime.js';
import type { CommentThread } from '@/shared/protocol.js';

export function GithubAnchorList({ threads, revision }: { threads: readonly CommentThread[]; revision: string }) {
  return (
    <>
      <p className="text-muted">
        {uiText('Commit')} <code>{revision.slice(0, 12)}</code>
      </p>
      <ul className="max-h-56 overflow-auto" aria-label={uiText('GitHub comment positions')}>
        {threads.map(({ id, anchor }) => (
          <li key={id} className="break-all">
            <code>{anchor.path}</code> ·{' '}
            {anchor.kind === 'file' ? (
              uiText('whole file')
            ) : (
              <code>
                {anchor.side === 'old' ? 'LEFT' : 'RIGHT'} L{anchor.startLine}
                {anchor.startLine !== anchor.endLine && `–${anchor.endLine}`}
              </code>
            )}
          </li>
        ))}
      </ul>
    </>
  );
}
