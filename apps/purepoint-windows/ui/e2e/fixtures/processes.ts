// Process helpers for the end-to-end harness (WIN-5.7).

import { execFile, type ChildProcess } from 'node:child_process';
import { promisify } from 'node:util';

const run = promisify(execFile);

/** Kill a process and everything it started (`taskkill /T /F`); a gone process is fine. */
export async function killTree(pid: number): Promise<void> {
  try {
    await run('taskkill', ['/PID', String(pid), '/T', '/F'], { windowsHide: true });
  } catch {
    // Already exited.
  }
}

/** Whether the child exits within `ms`. */
export function waitForExit(child: ChildProcess, ms: number): Promise<boolean> {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve(true);
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      child.off('exit', onExit);
      resolve(false);
    }, ms);
    const onExit = () => {
      clearTimeout(timer);
      resolve(true);
    };
    child.once('exit', onExit);
  });
}
