import { spawn } from 'node:child_process';
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
            const child = spawn(process.env.PU_SIGNAL_HELPER, ['__agent-signal'], { stdio: ['pipe','ignore','ignore'] });
            active++;
            let done = false;
            const finish = () => { if (!done) { done = true; active--; clearTimeout(timer); } };
            const timer = setTimeout(() => { child.kill(); finish(); }, 100); timer.unref();
            child.on('error', finish); child.on('close', finish);
            child.stdin.on('error', () => {}); child.stdin.end(JSON.stringify(clean));
          } catch { /* Observer failure never controls execution. */ }
        }
      } catch { /* Expiry gates observations if subscription is lost. */ }
    })();
    return () => abort.abort();
  }
};
