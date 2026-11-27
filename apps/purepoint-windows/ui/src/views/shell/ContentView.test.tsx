import { act, fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { useKeyBindingStore } from '../../state/keyBindingStore.ts';
import { useSidebarVisibilityStore } from '../../state/sidebarVisibilityStore.ts';
import { AppStateContext } from './appContext.ts';
import { ContentView, SidebarResizer } from './ContentView.tsx';
import {
  SIDEBAR_IDEAL_WIDTH,
  SIDEBAR_MAX_WIDTH,
  SIDEBAR_MIN_WIDTH,
  SIDEBAR_WIDTH_KEY,
  createSidebarWidthStore,
} from './sidebarWidth.ts';
import { makeTestApp, memoryStorage } from './testApp.ts';

describe('SidebarResizer', () => {
  it('drags the sidebar edge and persists the width across a relaunch', () => {
    const storage = memoryStorage();
    const store = createSidebarWidthStore(storage);
    render(<SidebarResizer store={store} />);
    const handle = screen.getByRole('separator', { name: 'Resize sidebar' });
    expect(handle).toHaveAttribute('aria-valuenow', String(SIDEBAR_IDEAL_WIDTH));

    fireEvent.pointerDown(handle, { button: 0, clientX: 240, pointerId: 1 });
    fireEvent.pointerMove(handle, { clientX: 270, pointerId: 1 });
    // A drag in progress resizes live but saves nothing yet.
    expect(store.getState().width).toBe(270);
    expect(storage.getItem(SIDEBAR_WIDTH_KEY)).toBeNull();
    fireEvent.pointerUp(handle, { clientX: 290, pointerId: 1 });
    expect(store.getState().width).toBe(290);
    expect(storage.getItem(SIDEBAR_WIDTH_KEY)).toBe('290');

    // The next launch reads it back.
    expect(createSidebarWidthStore(storage).getState().width).toBe(290);
  });

  it('clamps to the theme widths', () => {
    const storage = memoryStorage();
    const store = createSidebarWidthStore(storage);
    render(<SidebarResizer store={store} />);
    const handle = screen.getByRole('separator');
    fireEvent.pointerDown(handle, { button: 0, clientX: 240, pointerId: 1 });
    fireEvent.pointerUp(handle, { clientX: 900, pointerId: 1 });
    expect(store.getState().width).toBe(SIDEBAR_MAX_WIDTH);
    fireEvent.pointerDown(handle, { button: 0, clientX: 300, pointerId: 1 });
    fireEvent.pointerUp(handle, { clientX: 0, pointerId: 1 });
    expect(store.getState().width).toBe(SIDEBAR_MIN_WIDTH);
    storage.setItem(SIDEBAR_WIDTH_KEY, 'not a number');
    expect(createSidebarWidthStore(storage).getState().width).toBe(SIDEBAR_IDEAL_WIDTH);
  });

  it('resizes from the keyboard and resets on double-click', () => {
    const storage = memoryStorage();
    const store = createSidebarWidthStore(storage);
    render(<SidebarResizer store={store} />);
    const handle = screen.getByRole('separator');
    fireEvent.keyDown(handle, { key: 'ArrowRight' });
    expect(store.getState().width).toBe(SIDEBAR_IDEAL_WIDTH + 10);
    expect(storage.getItem(SIDEBAR_WIDTH_KEY)).toBe(String(SIDEBAR_IDEAL_WIDTH + 10));
    fireEvent.keyDown(handle, { key: 'Home' });
    expect(store.getState().width).toBe(SIDEBAR_MIN_WIDTH);
    fireEvent.doubleClick(handle);
    expect(store.getState().width).toBe(SIDEBAR_IDEAL_WIDTH);
  });
});

describe('ContentView', () => {
  beforeEach(() => {
    localStorage.clear();
    if (!useSidebarVisibilityStore.getState().sidebarShown) {
      useSidebarVisibilityStore.getState().toggleFor('toggleSidebar');
    }
    useKeyBindingStore.getState().resetAllBindings();
  });

  afterEach(() => {
    localStorage.clear();
  });

  function renderShell() {
    const app = makeTestApp();
    render(
      <AppStateContext.Provider value={app}>
        <ContentView />
      </AppStateContext.Provider>,
    );
    return app;
  }

  function runMenu(menu: string, item: RegExp) {
    fireEvent.click(screen.getByRole('button', { name: menu }));
    const dropdown = screen.getByRole('menu', { name: menu });
    fireEvent.click(within(dropdown).getByRole('menuitem', { name: item }));
  }

  it('lays out the sidebar, the detail and the title bar, starting on the dashboard', () => {
    renderShell();
    expect(screen.getByRole('navigation', { name: 'Sidebar' })).toBeInTheDocument();
    expect(screen.getByRole('separator', { name: 'Resize sidebar' })).toBeInTheDocument();
    expect(screen.getByTestId('point-guard-view')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Hide Sidebar' })).toBeInTheDocument();
  });

  it('routes nav selections to their screens', () => {
    renderShell();
    fireEvent.click(screen.getByRole('button', { name: 'Agents' }));
    expect(screen.getByTestId('agents-hub')).toBeInTheDocument();
    runMenu('Navigation', /^Schedule/);
    expect(screen.getByRole('tablist', { name: 'Schedule view' })).toBeInTheDocument();
  });

  it('shows menu accelerators from the key binding store', () => {
    renderShell();
    fireEvent.click(screen.getByRole('button', { name: 'File' }));
    const file = screen.getByRole('menu', { name: 'File' });
    expect(within(file).getByRole('menuitem', { name: /New Agent/ })).toHaveTextContent(
      'Ctrl+Shift+N',
    );
    expect(within(file).getByRole('menuitem', { name: /Exit/ })).toHaveTextContent('Alt+F4');
  });

  it('dispatches menu commands and their accelerators', () => {
    const app = renderShell();
    runMenu('View', /^Toggle Sidebar/);
    expect(screen.queryByRole('navigation', { name: 'Sidebar' })).toBeNull();
    expect(screen.getByRole('button', { name: 'Show Sidebar' })).toBeInTheDocument();

    // Ctrl+Shift+\ is Toggle Sidebar (DEC-8).
    fireEvent.keyDown(window, { code: 'Backslash', ctrlKey: true, shiftKey: true });
    expect(screen.getByRole('navigation', { name: 'Sidebar' })).toBeInTheDocument();

    // Alt+Shift+2 is Agents.
    fireEvent.keyDown(window, { code: 'Digit2', altKey: true, shiftKey: true });
    expect(screen.getByTestId('agents-hub')).toBeInTheDocument();

    // Ctrl+, opens Settings; Escape closes it.
    fireEvent.keyDown(window, { code: 'Comma', ctrlKey: true });
    expect(app.store.getState().showSettings).toBe(true);
    const dialog = screen.getByRole('dialog', { name: 'Settings' });
    fireEvent.keyDown(dialog, { key: 'Escape' });
    expect(app.store.getState().showSettings).toBe(false);
  });

  it('follows rebound accelerators', () => {
    renderShell();
    useKeyBindingStore.getState().setBinding({ code: 'KeyY', modifiers: ['alt'] }, 'navAgents');
    fireEvent.keyDown(window, { code: 'KeyY', altKey: true });
    expect(screen.getByTestId('agents-hub')).toBeInTheDocument();
  });

  it('runs pending commands as no-ops', () => {
    renderShell();
    runMenu('Panes', /^Split Right/);
    runMenu('Help', /^Check for Updates/);
    expect(screen.getByTestId('point-guard-view')).toBeInTheDocument();
  });

  it('shows the daemon banner over the shell', () => {
    const app = renderShell();
    act(() => {
      app.setDaemonError('pu-engine stopped');
    });
    expect(screen.getByRole('alert')).toHaveTextContent('pu-engine stopped');
  });

  it('closes an open menu on Escape and on a click elsewhere', () => {
    renderShell();
    fireEvent.click(screen.getByRole('button', { name: 'Tabs' }));
    const tabs = screen.getByRole('menu', { name: 'Tabs' });
    fireEvent.keyDown(tabs, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Tabs' }));
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole('menu')).toBeNull();
  });
});
