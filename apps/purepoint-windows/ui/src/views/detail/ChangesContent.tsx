// Swift `changesContent`: the review of a worktree or root checkout.
import type { DiffState } from '../../state/diffState.ts';
import { ReviewChangesView } from '../diff/ReviewChangesView.tsx';

export function ChangesContent({ state }: { state: DiffState }) {
  return (
    <div className="detail-changes">
      <ReviewChangesView state={state} />
    </div>
  );
}
