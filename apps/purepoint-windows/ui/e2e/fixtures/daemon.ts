// A real PurePoint daemon for one end-to-end spec (WIN-5.7).
//
// Each fixture owns a temp HOME (also USERPROFILE) holding a git repository, runs the
// workspace-built `pu.exe init` there, runs `pu-engine.exe` on that HOME's socket (so its pipe,
// `\\.\pipe\purepoint-<hash>-daemon`, is the one the app derives from the same HOME), spawns
// `terminal` agents, and tears all of it down. Nothing reads or writes the real `%USERPROFILE%`.

import { execFile, spawn, type ChildProcess } from 'node:child_process';
import { existsSync, mkdirSync, openSync, closeSync, realpathSync, rmSync } from 'node:fs';
import { writeFile } from 'node:fs/promises';
import { basename, dirname, join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { promisify } from 'node:util';
import { killTree, waitForExit } from './processes.ts';
import { workspace } from './paths.ts';

const run = promisify(execFile);

export interface CommandResult {
  readonly stdout: string;
  readonly stderr: string;
}

export interface DaemonFixtureOptions {
  /** Folder the fixture lives in; created, and removed by `dispose`. */
  readonly root: string;
  /** Name of the fixture project folder (what the sidebar shows). */
  readonly projectName?: string;
}

export class DaemonFixture {
  readonly root: string;
  readonly home: string;
  readonly projectRoot: string;
  readonly socket: string;
  readonly daemonLog: string;
  private engine: ChildProcess | null = null;

  private constructor(root: string, projectName: string) {
    this.root = root;
    this.home = join(root, 'home');
    this.projectRoot = join(this.home, projectName);
    this.socket = join(this.home, '.pu', 'daemon.sock');
    this.daemonLog = join(root, 'pu-engine.log');
  }

  /**
   * The fixture of the running spec, for specs that drive the daemon or the repository
   * themselves: wdio.conf.ts creates it in `beforeSession` and exports its HOME and project.
   */
  static current(): DaemonFixture {
    const home = process.env.PU_E2E_HOME;
    const projectRoot = process.env.PU_E2E_PROJECT_ROOT;
    if (home === undefined || projectRoot === undefined) {
      throw new Error('PU_E2E_HOME and PU_E2E_PROJECT_ROOT are not set');
    }
    return new DaemonFixture(dirname(home), basename(projectRoot));
  }

  /** Make the temp HOME and the git repository; nothing runs yet. */
  static async create(options: DaemonFixtureOptions): Promise<DaemonFixture> {
    mkdirSync(options.root, { recursive: true });
    // Long-name, canonical path: the pipe name hashes the socket path, so the app and the
    // daemon must see the same spelling (CI's temp folder is an 8.3 short name).
    const root = realpathSync.native(options.root);
    const fixture = new DaemonFixture(root, options.projectName ?? 'e2e-project');
    mkdirSync(join(fixture.home, '.pu'), { recursive: true });
    mkdirSync(fixture.projectRoot, { recursive: true });
    await fixture.git('init', '--quiet', '--initial-branch=main');
    await writeFile(join(fixture.projectRoot, 'README.md'), '# PurePoint e2e fixture\n');
    await fixture.git('add', 'README.md');
    await fixture.git('commit', '--quiet', '-m', 'Initial commit');
    return fixture;
  }

  /** The environment for anything that should use this fixture's HOME (the app included). */
  get env(): NodeJS.ProcessEnv {
    // Drop inherited PurePoint overrides so nothing points back at the real install.
    const env: NodeJS.ProcessEnv = Object.fromEntries(
      Object.entries(process.env).filter(([key]) => !/^PU_/i.test(key) || /^PU_E2E_/i.test(key)),
    );
    return {
      ...env,
      HOME: this.home,
      USERPROFILE: this.home,
      // Debug builds only: the app's installer leaves the registry PATH alone and skips `claude`.
      PU_INSTALLER_SANDBOX: '1',
      // Keep WebView2's profile (localStorage included) out of %LOCALAPPDATA%.
      WEBVIEW2_USER_DATA_FOLDER: join(this.root, 'webview2'),
      GIT_CONFIG_NOSYSTEM: '1',
      GIT_AUTHOR_NAME: 'PurePoint E2E',
      GIT_AUTHOR_EMAIL: 'e2e@purepoint.invalid',
      GIT_COMMITTER_NAME: 'PurePoint E2E',
      GIT_COMMITTER_EMAIL: 'e2e@purepoint.invalid',
    };
  }

  /** Run git in the project, or in `cwd` (a worktree). */
  async git(...args: string[]): Promise<CommandResult> {
    return this.gitIn(this.projectRoot, ...args);
  }

  async gitIn(cwd: string, ...args: string[]): Promise<CommandResult> {
    return run('git', args, { cwd, env: this.env, windowsHide: true });
  }

  /** Run the workspace-built `pu.exe` against this fixture's daemon. */
  async pu(args: readonly string[], cwd = this.projectRoot): Promise<CommandResult> {
    try {
      return await run(workspace.puExe, ['--socket', this.socket, ...args], {
        cwd,
        env: this.env,
        windowsHide: true,
        timeout: 60_000,
      });
    } catch (error) {
      const e = error as Error & { stdout?: string; stderr?: string };
      throw new Error(
        `pu ${args.join(' ')} failed: ${e.message}\n${e.stdout ?? ''}\n${e.stderr ?? ''}`,
        { cause: error },
      );
    }
  }

  /** Start `pu-engine.exe` on this HOME's socket and wait until it answers. */
  async startDaemon(): Promise<void> {
    if (this.engine !== null) return;
    const log = openSync(this.daemonLog, 'a');
    try {
      this.engine = spawn(workspace.engineExe, ['--socket', this.socket], {
        cwd: this.home,
        env: this.env,
        stdio: ['ignore', log, log],
        windowsHide: true,
      });
    } finally {
      closeSync(log);
    }
    const deadline = Date.now() + 30_000;
    while (Date.now() < deadline) {
      if (this.engine.exitCode !== null) {
        throw new Error(
          `pu-engine exited with ${String(this.engine.exitCode)}; see ${this.daemonLog}`,
        );
      }
      try {
        const { stdout } = await this.pu(['daemon', 'status', '--json']);
        if (/"pid"\s*:\s*\d+/.test(stdout)) return;
      } catch {
        // Not up yet.
      }
      await delay(250);
    }
    throw new Error(`pu-engine did not answer on ${this.socket} within 30 s`);
  }

  /** `pu init` in the fixture project, which registers it with the daemon. */
  async init(): Promise<void> {
    await this.pu(['init']);
  }

  /** Spawn a `terminal` agent in the project root; resolves its agent ID. */
  async spawnTerminal(): Promise<string> {
    const { stdout } = await this.pu(['spawn', '--root', '--agent', 'terminal', '--json']);
    const match = /"(?:agent_id|id)"\s*:\s*"([^"]+)"/.exec(stdout);
    if (!match?.[1]) throw new Error(`pu spawn printed no agent id: ${stdout}`);
    return match[1];
  }

  /** Spawn a `terminal` agent in a new worktree called `name`; resolves its agent ID. */
  async spawnWorktreeTerminal(name: string): Promise<string> {
    const { stdout } = await this.pu(['spawn', '--agent', 'terminal', '--name', name, '--json']);
    const match = /"(?:agent_id|id)"\s*:\s*"([^"]+)"/.exec(stdout);
    if (!match?.[1]) throw new Error(`pu spawn printed no agent id: ${stdout}`);
    return match[1];
  }

  /** Stop the daemon: `pu daemon stop`, then kill its tree if it lingers. */
  async stopDaemon(): Promise<void> {
    const engine = this.engine;
    if (engine === null) return;
    this.engine = null;
    if (engine.exitCode === null && engine.pid !== undefined) {
      try {
        await this.pu(['kill', '--all', '--include-root']);
      } catch {
        // No agents, or the daemon is already gone.
      }
      try {
        await this.pu(['daemon', 'stop']);
      } catch {
        // Killed below.
      }
      if (!(await waitForExit(engine, 15_000))) await killTree(engine.pid);
    }
  }

  /** Stop everything and delete the temp HOME. */
  async dispose(): Promise<void> {
    try {
      await this.stopDaemon();
    } finally {
      await removeTree(this.root);
    }
  }
}

/** `rm -rf` that rides out Windows' lingering file handles. */
export async function removeTree(path: string): Promise<void> {
  for (let attempt = 0; attempt < 10 && existsSync(path); attempt++) {
    try {
      rmSync(path, { recursive: true, force: true, maxRetries: 3, retryDelay: 200 });
    } catch {
      await delay(500);
    }
  }
}
