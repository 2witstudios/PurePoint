// Slow-rendering fake of Claude Code's input box, for delivery tests on
// Windows (ConPTY). Same behaviour and knobs as fake_tui.py, which needs
// termios:
//   READY_DELAY_MS    keystrokes are discarded and no box is drawn until this
//                     elapses (models Claude still loading hooks / first render)
//   PROCESS_DELAY_MS  each typed character becomes visible this long after it
//                     arrives (models a loaded machine); Enter is handled
//                     immediately on arrival, so it can submit a fragment
//   SWALLOW_ENTER     number of leading Enter presses to ignore
//   SUBMIT_LOG        file that receives one JSON line per submitted turn
import { appendFileSync } from 'node:fs';

const READY_DELAY = Number(process.env.READY_DELAY_MS ?? '0');
const PROCESS_DELAY = Number(process.env.PROCESS_DELAY_MS ?? '0');
let swallow = Number(process.env.SWALLOW_ENTER ?? '0');
const logPath = process.env.SUBMIT_LOG;
const COLS = 60;

const transcript = [];
const buf = [];
const pending = []; // [due time ms, char]
let ready = false;

const emit = (s) => process.stdout.write(s);

function draw() {
  const text = buf.join('');
  const rows = ['\x1b[2J\x1b[H'];
  for (const t of transcript.slice(-8)) rows.push('\x1b[1m> ' + t + '\x1b[0m\r\n');
  rows.push('╭' + '─'.repeat(COLS - 2) + '╮\r\n');
  const chunks = [];
  for (let i = 0; i < text.length; i += COLS - 6) chunks.push(text.slice(i, i + COLS - 6));
  if (!chunks.length) chunks.push('');
  chunks.forEach((c, i) => {
    rows.push('│ ' + (i === 0 ? '>' : ' ') + ' ' + c.padEnd(COLS - 6) + ' │\r\n');
  });
  rows.push('╰' + '─'.repeat(COLS - 2) + '╯\r\n');
  emit(rows.join(''));
}

function submit(text) {
  if (logPath) appendFileSync(logPath, JSON.stringify(text) + '\n');
  transcript.push(text);
  buf.length = 0;
  draw();
}

process.stdin.setRawMode(true);
process.stdin.setEncoding('utf8');
emit('loading hooks...\r\n');

process.stdin.on('data', (data) => {
  // Not ready: drop every byte that arrives.
  if (!ready) return;
  for (const ch of data) {
    if (ch === '\r') {
      if (swallow > 0) {
        swallow -= 1;
        continue;
      }
      submit(buf.join(''));
    } else if (ch === '\x15') {
      pending.length = 0;
      buf.length = 0;
      draw();
    } else if (ch === '\x7f' || ch === '\b') {
      if (buf.length) {
        buf.pop();
        draw();
      }
    } else if (ch >= ' ') {
      if (PROCESS_DELAY) {
        const last = pending.length ? pending[pending.length - 1][0] : Date.now();
        pending.push([Math.max(last, Date.now()) + PROCESS_DELAY, ch]);
      } else {
        buf.push(ch);
        draw();
      }
    }
  }
});

setTimeout(() => {
  ready = true;
  draw();
  setInterval(() => {
    const now = Date.now();
    let changed = false;
    while (pending.length && pending[0][0] <= now) {
      buf.push(pending.shift()[1]);
      changed = true;
    }
    if (changed) draw();
  }, 2);
}, READY_DELAY);
