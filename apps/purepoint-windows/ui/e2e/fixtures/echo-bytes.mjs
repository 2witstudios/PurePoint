// Terminal agent command for the terminal e2e spec (WIN-7.3). Launched by the daemon as a
// `terminal` agent (`pu spawn --agent terminal --command "node echo-bytes.mjs ..."`), so it runs
// in the agent's ConPTY with the pane's xterm.js on the other end.
//
//   node echo-bytes.mjs echo <log>
//     Raw stdin; turns on bracketed paste and prints a URL to Ctrl+click. Every chunk it reads
//     is printed as `RX <hex>` and appended to <log> as one hex line, so the spec can compare
//     it with what xterm.js encoded.
//     Ctrl+] (0x1d) toggles SGR mouse reporting (`CSI ? 1000 h` + `CSI ? 1006 h`).
//   node echo-bytes.mjs flood <bytesPerSecond> <totalBytes>
//     Waits for a key, then prints <totalBytes> of colored text at about <bytesPerSecond>
//     (0: as fast as the terminal takes it) and prints `FLOOD-END`.

import { appendFileSync, writeFileSync } from 'node:fs';

const [mode = 'echo', ...args] = process.argv.slice(2);
const ESC = '\x1b';

function rawStdin() {
  if (process.stdin.isTTY) process.stdin.setRawMode(true);
  process.stdin.resume();
}

function echo(log) {
  if (log) writeFileSync(log, '');
  let mouse = false;
  rawStdin();
  process.stdout.write(`${ESC}[?2004hECHO READY\r\nLINK https://example.com/pu-e2e?x=1\r\n`);
  process.stdin.on('data', (chunk) => {
    const hex = Buffer.from(chunk).toString('hex');
    if (log) appendFileSync(log, `${hex}\n`);
    process.stdout.write(`RX ${hex}\r\n`);
    if (chunk.length === 1 && chunk[0] === 0x1d) {
      mouse = !mouse;
      process.stdout.write(mouse ? `${ESC}[?1000h${ESC}[?1006h` : `${ESC}[?1006l${ESC}[?1000l`);
      process.stdout.write(`MOUSE ${mouse ? 'ON' : 'OFF'}\r\n`);
    }
  });
}

/** One line of the flood: a counter and a colored run, 100 bytes with CRLF. */
function floodLine(n) {
  const color = 31 + (n % 7);
  const text = `${String(n).padStart(9, '0')} ${ESC}[${color}m`;
  const tail = `${ESC}[0m\r\n`;
  const fill = 'abcdefghijklmnopqrstuvwxyz0123456789'.repeat(3);
  return text + fill.slice(0, 100 - text.length - tail.length) + tail;
}

function flood(rate, total) {
  rawStdin();
  process.stdout.write('FLOOD READY\r\n');
  process.stdin.once('data', () => {
    const chunkLines = 160; // 16 KB per write
    const started = Date.now();
    let sent = 0;
    let n = 0;
    const tick = () => {
      while (sent < total) {
        if (rate > 0 && sent > ((Date.now() - started) / 1000) * rate) {
          setTimeout(tick, 10);
          return;
        }
        let chunk = '';
        for (let i = 0; i < chunkLines; i++) chunk += floodLine(n++);
        sent += chunk.length;
        if (!process.stdout.write(chunk)) {
          process.stdout.once('drain', tick);
          return;
        }
      }
      process.stdout.write(`\r\nFLOOD-END ${String(sent)}\r\n`);
    };
    tick();
  });
}

if (mode === 'flood') {
  flood(Number(args[0] ?? 0), Number(args[1] ?? 1_000_000));
} else {
  echo(args[0]);
}
