// A fake `ssh.cmd` for the machines spec (WIN-8.14), the approach of
// crates/pu-cli/tests/machines_windows.rs: the app's relay runs `PU_SSH`, which answers the
// `uname -s` probe as a Windows machine would (no `uname`), runs the relay's `pu daemon proxy`
// with the workspace-built `pu.exe` against the "remote" fixture daemon's socket, and answers
// Start daemon's `pu health`. Every call's arguments go to `ssh.log` for the spec to read.

import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { workspace } from './paths.ts';

const FAKE_SSH = `@echo off
setlocal
echo %*>>"%FAKE_SSH_LOG%"
echo %* | findstr /c:"uname -s" >nul && goto uname
echo %* | findstr /c:"daemon proxy" >nul && goto proxy
echo %* | findstr /c:"pu health" >nul && goto health
echo unexpected ssh invocation: %* 1>&2
exit /b 255
:uname
echo 'uname' is not recognized as an internal or external command 1>&2
exit /b 1
:proxy
"%FAKE_PU%" --socket "%FAKE_REMOTE_SOCKET%" daemon proxy
exit /b %ERRORLEVEL%
:health
echo remote daemon started
exit /b 0
`;

/** Write the fake into `dir`; returns the environment that points the app at it. */
export function writeFakeSsh(dir: string, remoteSocket: string): NodeJS.ProcessEnv {
  mkdirSync(dir, { recursive: true });
  const ssh = join(dir, 'ssh.cmd');
  writeFileSync(ssh, FAKE_SSH.replace(/\r?\n/g, '\r\n'));
  return {
    PU_SSH: ssh,
    FAKE_SSH_LOG: join(dir, 'ssh.log'),
    FAKE_PU: workspace.puExe,
    FAKE_REMOTE_SOCKET: remoteSocket,
  };
}

/** The fake's log: one line of arguments per ssh call. */
export function fakeSshLog(dir: string): string {
  return join(dir, 'ssh.log');
}
