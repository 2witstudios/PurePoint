// The remote folder picker's browsing rules (WIN-8.14), after the iPad's RemoteFolderBrowserTests:
// home first, hidden folders off, Finder's order, and the Mac panel's history.
import { describe, expect, it } from 'vitest';
import type { Response } from '../protocol';
import { RemoteFolderBrowser, parentFolder, sortFolders } from './remoteFolderBrowser';

class FakeMachineFolders {
  tree: Record<string, string[]> = {
    '/home/me': ['code', '.config', 'Documents'],
    '/home/me/code': ['app', 'Zeta', 'beta', 'alpha2', 'alpha10'],
    '/home/me/code/app': ['src'],
    '/home': ['me'],
    '/': ['home', 'srv'],
  };
  symlinks = new Set(['/home/me/Documents']);
  requests: { path: string; includeHidden: boolean }[] = [];
  failure: Response | null = null;

  list = (path: string, includeHidden: boolean): Promise<Response> => {
    this.requests.push({ path, includeHidden });
    if (this.failure !== null) return Promise.resolve(this.failure);
    const resolved = path === '~' ? '/home/me' : path;
    const names = this.tree[resolved];
    if (names === undefined) {
      return Promise.resolve({
        type: 'error',
        code: 'not_found',
        message: `${resolved} does not exist`,
      });
    }
    const entries = names
      .filter((name) => includeHidden || !name.startsWith('.'))
      .map((name) => {
        const child = resolved === '/' ? `/${name}` : `${resolved}/${name}`;
        return {
          name,
          path: child,
          is_dir: true,
          is_symlink: this.symlinks.has(child),
          size: 0,
          modified_ms: 0,
          ignored: false,
        };
      });
    return Promise.resolve({ type: 'fs_list_result', path: resolved, entries, truncated: false });
  };
}

function browser() {
  const folders = new FakeMachineFolders();
  return { browser: new RemoteFolderBrowser(folders.list), folders };
}

const names = (b: RemoteFolderBrowser) => b.state.folders.map((folder) => folder.name);

describe('RemoteFolderBrowser', () => {
  it('given a machine, should start at the home folder with visible folders only', async () => {
    const { browser: b, folders } = browser();
    await b.start();
    expect(folders.requests[0]).toEqual({ path: '~', includeHidden: false });
    expect(b.state.path).toBe('/home/me');
    expect(b.state.home).toBe('/home/me');
    expect(names(b)).toEqual(['code', 'Documents']);
    expect(b.state.folders.at(-1)?.isSymlink).toBe(true);
    expect(b.state.selection).toBeNull();
    expect(b.state.backStack).toEqual([]);
  });

  it('given folders, should sort them like Finder', async () => {
    const { browser: b } = browser();
    await b.start();
    await b.navigate('/home/me/code');
    expect(names(b)).toEqual(['alpha2', 'alpha10', 'app', 'beta', 'Zeta']);
    expect(sortFolders([]).length).toBe(0);
  });

  it('given show hidden, should list again keeping the selection', async () => {
    const { browser: b, folders } = browser();
    await b.start();
    b.select('/home/me/code');
    await b.setShowHidden(true);
    expect(folders.requests.at(-1)?.includeHidden).toBe(true);
    expect(names(b)).toEqual(['.config', 'code', 'Documents']);
    expect(b.state.selection).toBe('/home/me/code');
    b.select('/home/me/.config');
    await b.setShowHidden(false);
    expect(b.state.selection).toBeNull();
  });

  it('given enter, up, back and forward, should keep the Mac panel history', async () => {
    const { browser: b } = browser();
    await b.start();
    const code = b.state.folders[0];
    if (code === undefined) throw new Error('no folders');
    await b.enter(code);
    expect(b.state.path).toBe('/home/me/code');
    expect(b.state.backStack).toEqual(['/home/me']);
    await b.goUp();
    // The enclosing folder selects the one just left.
    expect(b.state.path).toBe('/home/me');
    expect(b.state.selection).toBe('/home/me/code');
    await b.goBack();
    expect(b.state.path).toBe('/home/me/code');
    expect(b.state.forwardStack).toEqual(['/home/me']);
    await b.goForward();
    expect(b.state.path).toBe('/home/me');
    expect(b.state.forwardStack).toEqual([]);
    expect(b.chosenPath).toBe('/home/me');
    b.select('/home/me/Documents');
    expect(b.chosenPath).toBe('/home/me/Documents');
  });

  it("given the daemon's refusal, should show its message and stay put", async () => {
    const { browser: b } = browser();
    await b.start();
    await b.navigate('/missing');
    expect(b.state.path).toBe('/home/me');
    expect(b.state.error).toBe('/missing does not exist');
    expect(b.state.backStack).toEqual([]);
  });

  it('should find the enclosing folder on Unix and Windows machines', () => {
    expect(parentFolder('/home/me')).toBe('/home');
    expect(parentFolder('/home')).toBe('/');
    expect(parentFolder('/')).toBeNull();
    expect(parentFolder('C:\\Users\\me')).toBe('C:\\Users');
    expect(parentFolder('C:\\Users')).toBe('C:\\');
    expect(parentFolder('C:\\')).toBeNull();
    expect(parentFolder('/srv/app/')).toBe('/srv');
  });
});
