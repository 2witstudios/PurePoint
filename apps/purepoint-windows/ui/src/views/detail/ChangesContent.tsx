// Swift `changesContent`: the review of a worktree or root checkout. ReviewChangesView is WIN-8.3.
import type { DiffState } from '../../state/diffState.ts';
import { ScreenPlaceholder } from './placeholders.tsx';

export function ChangesContent({ state }: { state: DiffState }) {
  return (
    <div className="detail-changes" data-identity={state.reviewIdentity}>
      <ScreenPlaceholder icon="doc.badge.plus" title="Changes" view="ReviewChangesView" />
    </div>
  );
}
