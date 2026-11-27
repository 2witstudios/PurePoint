import { describe, expect, it } from 'vitest';
import { MemoryStorage } from '../model/sidebarFixtures.ts';
import {
  SIDEBAR_EXPANSION_STORAGE_KEY,
  createSidebarExpansionStore,
} from './sidebarExpansionStore.ts';

/**
 * Sidebar expansion has to outlive the rows it describes: the tree is rebuilt on every data
 * refresh, so what is closed is recorded outside it. Each test uses its own storage, as the Swift
 * suite uses a private `UserDefaults` suite. (Port of SidebarExpansionStoreTests.swift.)
 */
describe('SidebarExpansionStoreTests', () => {
  it('givenNothingRecordedShouldClaimNoRowsAreClosed', () => {
    const store = createSidebarExpansionStore(new MemoryStorage()).getState();

    expect(store.closedRowKeys.size).toBe(0);
  });

  it('givenARowTheUserClosedShouldStayClosedAcrossALaunch', () => {
    const storage = new MemoryStorage();
    const store = createSidebarExpansionStore(storage).getState();
    store.setExpanded('project:%2Fa', true);
    store.setExpanded('project:%2Fa', false);
    store.setExpanded('project:%2Fb', false);

    const relaunched = createSidebarExpansionStore(storage).getState();
    expect(relaunched.closedRowKeys).toEqual(new Set(['project:%2Fa', 'project:%2Fb']));
  });

  it('givenARowTheUserReopenedShouldNotStayClosed', () => {
    const storage = new MemoryStorage();
    const store = createSidebarExpansionStore(storage);
    store.getState().setExpanded('project:%2Fa', false);
    store.getState().setExpanded('project:%2Fa', true);

    expect(store.getState().closedRowKeys.size).toBe(0);
    const relaunched = createSidebarExpansionStore(storage).getState();
    expect(relaunched.closedRowKeys.size).toBe(0);
  });

  it('givenARowThatNoLongerExistsShouldDropItWhenTheTreeIsRecorded', () => {
    const storage = new MemoryStorage();
    const store = createSidebarExpansionStore(storage);
    store.getState().record(new Set(['project:%2Fgone', 'project:%2Fkept']));

    // The tree as it stands now: only the kept row still exists.
    store.getState().record(new Set(['project:%2Fkept']));

    expect(store.getState().closedRowKeys).toEqual(new Set(['project:%2Fkept']));
    const relaunched = createSidebarExpansionStore(storage).getState();
    expect(relaunched.closedRowKeys).toEqual(new Set(['project:%2Fkept']));
  });

  it('givenTheSameRowsRecordedAgainShouldNotRewriteThem', () => {
    const storage = new MemoryStorage();
    const store = createSidebarExpansionStore(storage);
    store.getState().record(new Set(['project:%2Fa']));
    const writes = storage.writes;

    store.getState().record(new Set(['project:%2Fa']));

    expect(storage.writes).toBe(writes);
    expect(JSON.parse(storage.getItem(SIDEBAR_EXPANSION_STORAGE_KEY) ?? '')).toEqual([
      'project:%2Fa',
    ]);
  });
});
