// A second attach client for the terminal e2e spec (WIN-7.3): speaks the daemon protocol over
// the fixture's pipe, as the app's attach session does, and records the output bytes it is sent.
// The daemon sends every attached client the same live stream, so the bytes this tap counts
// from a quiet point on are the bytes the app's pane must have received.

import { createHash } from 'node:crypto';
import { connect, type Socket } from 'node:net';
import { join } from 'node:path';
import type { DaemonFixture } from './daemon.ts';

const FNV_OFFSET = 0x811c9dc5;
const FNV_PRIME = 0x01000193;

/** The pane's checksum (ui/src/views/terminal/terminalStats.ts). */
export function fnv1a(hash: number, bytes: Uint8Array): number {
  let h = hash >>> 0;
  for (const byte of bytes) {
    h ^= byte;
    h = Math.imul(h, FNV_PRIME);
  }
  return h >>> 0;
}

/** DEC-2 pipe name for a socket path whose parent is already canonical (pu-transport naming.rs). */
export function daemonPipe(socketPath: string): string {
  let normalized = socketPath.replace(/\//g, '\\');
  if (/^[a-z]:/.test(normalized))
    normalized = normalized.charAt(0).toUpperCase() + normalized.slice(1);
  const hash = createHash('sha256').update(normalized, 'utf8').digest().subarray(0, 8);
  return `\\\\.\\pipe\\purepoint-${hash.toString('hex')}-daemon`;
}

export class AttachTap {
  bytes = 0;
  checksum = FNV_OFFSET;
  /** The last 64 KiB of output as Latin-1, to look for markers in. */
  private tail = '';
  private readonly socket: Socket;
  private pending = '';

  private constructor(socket: Socket) {
    this.socket = socket;
    socket.setEncoding('utf8');
    socket.on('data', (chunk: string) => {
      this.pending += chunk;
      let newline = this.pending.indexOf('\n');
      while (newline >= 0) {
        const line = this.pending.slice(0, newline);
        this.pending = this.pending.slice(newline + 1);
        this.handle(line);
        newline = this.pending.indexOf('\n');
      }
    });
  }

  /** Attach to `agentId`; with `clientId` the tap can claim the PTY size like a client does. */
  static async open(
    fixture: DaemonFixture,
    agentId: string,
    clientId?: string,
  ): Promise<AttachTap> {
    const pipe = daemonPipe(join(fixture.home, '.pu', 'daemon.sock'));
    const socket = await new Promise<Socket>((resolve, reject) => {
      const s = connect(pipe, () => {
        resolve(s);
      });
      s.once('error', reject);
    });
    const tap = new AttachTap(socket);
    tap.send({
      type: 'attach',
      agent_id: agentId,
      replay: 'snapshot',
      ...(clientId === undefined ? {} : { client_id: clientId }),
    });
    return tap;
  }

  private send(request: Record<string, unknown>): void {
    this.socket.write(`${JSON.stringify(request)}\n`);
  }

  private handle(line: string): void {
    if (line.trim() === '') return;
    let message: { type?: string; data?: string };
    try {
      message = JSON.parse(line) as { type?: string; data?: string };
    } catch {
      return;
    }
    if (message.type !== 'output' || typeof message.data !== 'string') return;
    const bytes = Buffer.from(message.data, 'hex');
    this.bytes += bytes.length;
    this.checksum = fnv1a(this.checksum, bytes);
    this.tail = (this.tail + bytes.toString('latin1')).slice(-65_536);
  }

  /** Count from zero from here on. */
  reset(): void {
    this.bytes = 0;
    this.checksum = FNV_OFFSET;
    this.tail = '';
  }

  sawText(text: string): boolean {
    return this.tail.includes(text);
  }

  claimSize(cols: number, rows: number): void {
    this.send({ type: 'claim_size', cols, rows });
  }

  close(): void {
    this.socket.destroy();
  }
}
