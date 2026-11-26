import { spawn } from 'node:child_process';
import net from 'node:net';
// Windows launches name a named pipe, written directly; Unix keeps the helper's datagram.
const PIPE = (process.env.PU_SIGNAL_SOCKET ?? '').startsWith('\\\\.\\pipe\\');
// No policy callbacks, UI wrappers, remote calls or awaited relay operations.
export default function (pi) {
  let sequence = 0, outcome, request = 0, pending, running = false, lost = false, last;
  const queue = [];
  const drain = () => {
    if (running) return;
    if (!queue.length && lost && last) {
      lost = false;
      queue.push(JSON.stringify({ ...last, type: 'observer_loss', sequence: ++sequence, occurred_at_ms: Date.now() }));
    }
    const data = queue.shift();
    if (!data) return;
    running = true;
    const lossReport = JSON.parse(data).type === 'observer_loss';
    try {
      let done = false, stop;
      const finish = () => { if (!done) { done = true; running = false; clearTimeout(timer); queueMicrotask(drain); } };
      if (PIPE) {
        // Windows: one {"token","payload"} frame per pipe connection, ended by EOF.
        const frame = JSON.stringify({ token: process.env.PU_SIGNAL_TOKEN ?? '', payload: JSON.parse(data) });
        const socket = net.createConnection(process.env.PU_SIGNAL_SOCKET, () => socket.end(frame));
        stop = () => socket.destroy();
        socket.on('error', finish); socket.on('close', finish);
      } else {
        const child = spawn(process.env.PU_SIGNAL_HELPER, ['__agent-signal'], { stdio: ['pipe', 'ignore', 'ignore'] });
        stop = () => child.kill();
        child.on('error', finish); child.on('close', finish);
        child.stdin.on('error', () => {}); child.stdin.end(data);
      }
      const timer = setTimeout(() => { stop(); lost = !lossReport; finish(); }, 100);
      timer.unref();
    } catch { running = false; lost = !lossReport; queueMicrotask(drain); }
  };
  const send = (payload, ctx) => {
    const seq = ++sequence;
    try {
      last = { session_id: ctx.sessionManager.getSessionId(), native_pid: process.pid };
      if (queue.length >= 64) { lost = true; return; }
      queue.push(JSON.stringify({ ...payload, ...last, sequence: seq, occurred_at_ms: Date.now() }));
      drain();
    } catch { lost = true; }
  };
  pi.on('session_start', (_, ctx) => send({ type: 'observer_bound' }, ctx));
  pi.on('agent_start', (_, ctx) => { outcome = undefined; send({ type: 'agent_start' }, ctx); });
  pi.on('agent_before_settle', (event) => { outcome = event.outcome; });
  pi.on('agent_settled', (event, ctx) => send({ type: event.type, aborted: event.aborted, outcome }, ctx));
  pi.on('ui_prompt_start', (event, ctx) => { pending = `ui-${++request}`; send({ type: event.type, kind: event.kind, request_id: pending }, ctx); });
  pi.on('ui_prompt_end', (event, ctx) => { if (pending) send({ type: event.type, kind: event.kind, request_id: pending }, ctx); pending = undefined; });
}
