import { spawn } from 'node:child_process';
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
      const child = spawn(process.env.PU_SIGNAL_HELPER, ['__agent-signal'], { stdio: ['pipe', 'ignore', 'ignore'] });
      let done = false;
      const finish = () => { if (!done) { done = true; running = false; clearTimeout(timer); queueMicrotask(drain); } };
      const timer = setTimeout(() => { child.kill(); lost = !lossReport; finish(); }, 100);
      timer.unref();
      child.on('error', finish); child.on('close', finish);
      child.stdin.on('error', () => {}); child.stdin.end(data);
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
