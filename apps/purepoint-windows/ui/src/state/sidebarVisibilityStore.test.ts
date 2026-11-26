import { describe, expect, it } from 'vitest';
import { MemoryStorage } from '../model/sidebarFixtures.ts';
import {
  HOTKEY_ACTIONS,
  actionDisplayName,
  actionsFor,
  chordId,
  defaultBinding,
  keyBinding,
} from '../model/keyBinding.ts';
import {
  SIDEBAR_KEY,
  WORKSPACE_SIDEBAR_KEY,
  createSidebarVisibilityStore,
  isSidebarShown,
  toggleSidebarKey,
} from './sidebarVisibilityStore.ts';

/**
 * Both sidebars' shown/hidden state outlives a relaunch, and the hotkey that toggles the workspace
 * sidebar is listed and bound like the others. (Port of SidebarVisibilityTests.swift; the default
 * chord is the DEC-8 Windows one.)
 */
describe('SidebarVisibilityTests', () => {
  it('givenNothingStoredShouldShowBothSidebars', () => {
    const storage = new MemoryStorage();

    expect(isSidebarShown(SIDEBAR_KEY, storage)).toBe(true);
    expect(isSidebarShown(WORKSPACE_SIDEBAR_KEY, storage)).toBe(true);
  });

  it('givenASidebarHiddenShouldStayHiddenAcrossALaunch', () => {
    const storage = new MemoryStorage();
    toggleSidebarKey(SIDEBAR_KEY, storage);

    const relaunched = createSidebarVisibilityStore(storage).getState();
    expect(relaunched.sidebarShown).toBe(false);
    expect(isSidebarShown(SIDEBAR_KEY, storage)).toBe(false);
  });

  it('givenAToggleTwiceShouldShowTheSidebarAgain', () => {
    const storage = new MemoryStorage();
    toggleSidebarKey(WORKSPACE_SIDEBAR_KEY, storage);
    toggleSidebarKey(WORKSPACE_SIDEBAR_KEY, storage);

    expect(isSidebarShown(WORKSPACE_SIDEBAR_KEY, storage)).toBe(true);
  });

  it('givenOneSidebarToggledShouldLeaveTheOtherAlone', () => {
    const storage = new MemoryStorage();
    toggleSidebarKey(WORKSPACE_SIDEBAR_KEY, storage);

    expect(isSidebarShown(SIDEBAR_KEY, storage)).toBe(true);
  });

  it('givenASidebarToggleActionShouldFlipItsSidebarOnce', () => {
    const storage = new MemoryStorage();
    const store = createSidebarVisibilityStore(storage);

    expect(store.getState().toggleFor('toggleWorkspaceSidebar')).toBe(true);
    expect(isSidebarShown(WORKSPACE_SIDEBAR_KEY, storage)).toBe(false);
    expect(store.getState().workspaceSidebarShown).toBe(false);
    expect(isSidebarShown(SIDEBAR_KEY, storage)).toBe(true);

    expect(store.getState().toggleFor('toggleSidebar')).toBe(true);
    expect(isSidebarShown(SIDEBAR_KEY, storage)).toBe(false);
    expect(store.getState().sidebarShown).toBe(false);
  });

  it('givenAnotherActionShouldLeaveBothSidebarsAlone', () => {
    const storage = new MemoryStorage();
    const store = createSidebarVisibilityStore(storage);

    expect(store.getState().toggleFor('focusSidebar')).toBe(false);
    expect(isSidebarShown(SIDEBAR_KEY, storage)).toBe(true);
    expect(isSidebarShown(WORKSPACE_SIDEBAR_KEY, storage)).toBe(true);
  });

  it('givenTheWorkspaceSidebarHotkeyShouldBeListedUnderNavigation', () => {
    expect(actionsFor('navigation')).toContain('toggleWorkspaceSidebar');
    expect(actionDisplayName('toggleWorkspaceSidebar')).toBe('Toggle Workspace Sidebar');
  });

  it('givenDefaultBindingsShouldNotClashWithEachOther', () => {
    const bindings = HOTKEY_ACTIONS.map(defaultBinding);
    expect(new Set(bindings.map(chordId)).size).toBe(bindings.length);
    expect(defaultBinding('toggleWorkspaceSidebar')).toEqual(
      keyBinding('Backslash', 'alt', 'shift'),
    );
  });
});
