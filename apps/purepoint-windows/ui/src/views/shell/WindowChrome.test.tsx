import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { emit } from '@tauri-apps/api/event';
import { clearMocks, mockIPC, mockWindows } from '@tauri-apps/api/mocks';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { CAPTION_BUTTONS_EVENT } from '../../services/windowChrome.ts';
import { WindowChrome, measureChrome } from './WindowChrome.tsx';

interface Call {
  cmd: string;
  payload: unknown;
}

let calls: Call[];
let maximized: boolean;

beforeEach(() => {
  calls = [];
  maximized = false;
  mockWindows('main');
  mockIPC(
    (cmd, payload) => {
      calls.push({ cmd, payload });
      if (cmd === 'plugin:window|is_maximized') return maximized;
      return null;
    },
    { shouldMockEvents: true },
  );
});

afterEach(() => {
  cleanup();
  clearMocks();
});

const commands = () => calls.map((call) => call.cmd);

function renderChrome() {
  render(
    <WindowChrome>
      <button type="button" data-chrome-interactive>
        File
      </button>
    </WindowChrome>,
  );
}

describe('WindowChrome caption buttons', () => {
  it('minimize, maximize and close invoke the window commands', async () => {
    renderChrome();
    fireEvent.click(screen.getByRole('button', { name: 'Minimize' }));
    fireEvent.click(screen.getByRole('button', { name: 'Maximize' }));
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    await waitFor(() => {
      expect(commands()).toEqual(
        expect.arrayContaining([
          'plugin:window|minimize',
          'plugin:window|toggle_maximize',
          'plugin:window|close',
        ]),
      );
    });
    const order = commands().filter((cmd) =>
      ['plugin:window|minimize', 'plugin:window|toggle_maximize', 'plugin:window|close'].includes(
        cmd,
      ),
    );
    expect(order).toEqual([
      'plugin:window|minimize',
      'plugin:window|toggle_maximize',
      'plugin:window|close',
    ]);
  });

  it('shows Restore with the restore glyph while maximized', async () => {
    maximized = true;
    renderChrome();
    const restore = await screen.findByRole('button', { name: 'Restore' });
    expect(restore).toHaveTextContent('');
  });

  it('paints the hover and pressed states the backend reports', async () => {
    renderChrome();
    const maximize = screen.getByRole('button', { name: 'Maximize' });
    // Let the listener register.
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    await act(async () => {
      await emit(CAPTION_BUTTONS_EVENT, { maximizeHover: true, maximizePressed: false });
    });
    expect(maximize).toHaveClass('is-hover');
    expect(maximize).not.toHaveClass('is-pressed');
    await act(async () => {
      await emit(CAPTION_BUTTONS_EVENT, { maximizeHover: true, maximizePressed: true });
    });
    expect(maximize).toHaveClass('is-pressed');
    await act(async () => {
      await emit(CAPTION_BUTTONS_EVENT, { maximizeHover: false, maximizePressed: false });
    });
    expect(maximize).not.toHaveClass('is-hover');
  });

  it('reports its layout to the backend for hit-testing', async () => {
    renderChrome();
    await waitFor(() => {
      expect(commands()).toContain('chrome_set_layout');
    });
    const call = calls.find((c) => c.cmd === 'chrome_set_layout');
    const layout = (call?.payload as { layout: { interactive: unknown[] } }).layout;
    expect(layout).toMatchObject({
      caption: { x: 0, y: 0 },
      maximize: { x: 0, y: 0 },
    });
    // The menu button, minimize and close keep their clicks; maximize is separate.
    expect(layout.interactive).toHaveLength(3);
  });
});

describe('measureChrome', () => {
  it('needs a maximize button', () => {
    const header = document.createElement('header');
    expect(measureChrome(header)).toBeNull();
  });
});
