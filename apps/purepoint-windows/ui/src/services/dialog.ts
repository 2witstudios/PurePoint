/**
 * Native file dialogs (WIN-8.15, REQ-WINAPP-074): the Tauri dialog plugin's folder picker (the
 * Windows `IFileOpenDialog` with `FOS_PICKFOLDERS`), where the Mac runs an `NSOpenPanel` with
 * `canChooseDirectories` and no files.
 */
import { open } from '@tauri-apps/plugin-dialog';
import { testHooksEnabled } from './testHooks';

interface DialogSpyGlobal {
  /**
   * Set by the end-to-end tests only (read in debug builds only, `__PU_TEST_HOOKS__`): each folder pick takes the next entry instead of showing
   * the dialog (WebDriver cannot drive a native window). `null` stands for Cancel; an empty
   * queue cancels too.
   */
  __PU_DIALOG_SPY__?: (string | null)[];
}

export interface PickFolderOptions {
  /** The dialog's title (the Mac panel's `message`). */
  readonly title?: string;
  /** The folder it opens in. */
  readonly defaultPath?: string;
}

/** Ask for one folder; resolves its path, or null when the user cancels. */
export async function pickFolder(options: PickFolderOptions = {}): Promise<string | null> {
  const queue = testHooksEnabled() ? (globalThis as DialogSpyGlobal).__PU_DIALOG_SPY__ : undefined;
  if (Array.isArray(queue)) return queue.shift() ?? null;
  const picked = await open({
    directory: true,
    multiple: false,
    ...(options.title === undefined ? {} : { title: options.title }),
    ...(options.defaultPath === undefined ? {} : { defaultPath: options.defaultPath }),
  });
  return typeof picked === 'string' ? picked : null;
}
