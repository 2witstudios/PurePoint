// `tauri-driver` (and the `msedgedriver` it runs) for one spec (WIN-5.7).
//
// The app inherits its environment from msedgedriver, which inherits it from tauri-driver, so
// the driver starts per spec with the fixture's HOME. Stopping kills the whole tree: the app,
// its WebView2 processes and msedgedriver.

import { spawn, type ChildProcess } from 'node:child_process';
import { closeSync, openSync, readFileSync } from 'node:fs';
import { createServer, connect } from 'node:net';
import { join } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { killTree, waitForExit } from './processes.ts';
import { workspace } from './paths.ts';

export interface Drivers {
  readonly webview2Version: string;
  readonly msedgedriver: string;
  readonly tauriDriver: string;
  readonly tauriDriverVersion: string;
}

/** What ui/e2e/setup-drivers.ps1 installed. */
export function readDrivers(): Drivers {
  const path = join(workspace.toolsDir, 'drivers.json');
  // PowerShell 5.1 may write a BOM.
  const text = readFileSync(path, 'utf8');
  return JSON.parse(text.charCodeAt(0) === 0xfeff ? text.slice(1) : text) as Drivers;
}

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.unref();
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      const port = typeof address === 'object' && address !== null ? address.port : 0;
      server.close(() => {
        resolve(port);
      });
    });
  });
}

function canConnect(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = connect({ host: '127.0.0.1', port });
    socket.once('connect', () => {
      socket.destroy();
      resolve(true);
    });
    socket.once('error', () => {
      resolve(false);
    });
  });
}

export class TauriDriver {
  private constructor(
    private child: ChildProcess | null,
    readonly port: number,
  ) {}

  static async start(env: NodeJS.ProcessEnv, logPath: string): Promise<TauriDriver> {
    const drivers = readDrivers();
    const port = await freePort();
    let nativePort = await freePort();
    while (nativePort === port) nativePort = await freePort();
    const log = openSync(logPath, 'a');
    let child: ChildProcess;
    try {
      child = spawn(
        drivers.tauriDriver,
        [
          '--port',
          String(port),
          '--native-port',
          String(nativePort),
          '--native-driver',
          drivers.msedgedriver,
        ],
        { env, stdio: ['ignore', log, log], windowsHide: true },
      );
    } finally {
      closeSync(log);
    }
    const driver = new TauriDriver(child, port);
    const deadline = Date.now() + 20_000;
    while (Date.now() < deadline) {
      if (child.exitCode !== null) {
        throw new Error(`tauri-driver exited with ${String(child.exitCode)}; see ${logPath}`);
      }
      if (await canConnect(port)) return driver;
      await delay(200);
    }
    await driver.stop();
    throw new Error(`tauri-driver did not listen on ${String(port)} within 20 s`);
  }

  /** Kill tauri-driver, msedgedriver and the app they launched. */
  async stop(): Promise<void> {
    const child = this.child;
    this.child = null;
    if (child?.pid === undefined) return;
    await killTree(child.pid);
    await waitForExit(child, 5_000);
  }
}
