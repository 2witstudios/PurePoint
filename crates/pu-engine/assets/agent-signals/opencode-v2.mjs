import { spawn } from 'node:child_process';
import net from 'node:net';
// Windows launches name a named pipe, written directly; Unix keeps the helper's datagram.
const PIPE = (process.env.PU_SIGNAL_SOCKET ?? '').startsWith('\\\\.\\pipe\\');
// Pinned v2 Promise plugin. Fixture-qualified only: default managed-service
// reuse does not carry per-launch config/env into an already running service.
export default {
  id: 'purepoint.agent-signals.v1',
  setup(context) {
    const abort = new AbortController();
    let sequence = 0, active = 0;
    void (async () => {
      try {
        for await (const event of context.event.subscribe({ signal: abort.signal })) {
          const data = event.data;
          if ((data?.sessionID ?? data?.form?.sessionID) !== process.env.PU_SIGNAL_SESSION) continue;
          if (!['session.execution.started','session.execution.succeeded','session.execution.failed','session.execution.interrupted','permission.asked','permission.replied','form.created','form.replied','form.cancelled'].includes(event.type)) continue;
          const seq = ++sequence;
          if (active >= 2) continue;
          const clean = { id: event.id, type: event.type, sequence: seq, occurred_at_ms: Date.now(), data: { sessionID: data.sessionID, id: data.id, requestID: data.requestID, form: data.form ? { id: data.form.id, sessionID: data.form.sessionID } : undefined } };
          try {
            let done = false, stop;
            const finish = () => { if (!done) { done = true; active--; clearTimeout(timer); } };
            if (PIPE) {
              // Windows: one {"token","payload"} frame per pipe connection, ended by EOF.
              const frame = JSON.stringify({ token: process.env.PU_SIGNAL_TOKEN ?? '', payload: clean });
              const socket = net.createConnection(process.env.PU_SIGNAL_SOCKET, () => socket.end(frame));
              stop = () => socket.destroy();
              socket.on('error', finish); socket.on('close', finish);
            } else {
              const child = spawn(process.env.PU_SIGNAL_HELPER, ['__agent-signal'], { stdio: ['pipe','ignore','ignore'] });
              stop = () => child.kill();
              child.on('error', finish); child.on('close', finish);
              child.stdin.on('error', () => {}); child.stdin.end(JSON.stringify(clean));
            }
            active++;
            const timer = setTimeout(() => { stop(); finish(); }, 100); timer.unref();
          } catch { /* Observer failure never controls execution. */ }
        }
      } catch { /* Expiry gates observations if subscription is lost. */ }
    })();
    return () => abort.abort();
  }
};
