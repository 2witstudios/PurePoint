// Writes one signal frame per connection to the per-launch signal pipe, the
// Windows replacement for the UnixDatagram send. Env: PU_SIGNAL_PIPE,
// PU_SIGNAL_TOKEN. argv[2]: JSON payload. EOF delimits the message.
import net from 'node:net';

const payload = JSON.parse(process.argv[2] ?? '{}');
payload.occurred_at_ms ??= Date.now();
const frame = JSON.stringify({ token: process.env.PU_SIGNAL_TOKEN ?? '', payload });
const timer = setTimeout(() => process.exit(3), 1000);
const sock = net.createConnection(process.env.PU_SIGNAL_PIPE, () => sock.end(frame));
sock.on('error', (e) => { console.error(e.message); process.exitCode = 1; });
sock.on('close', () => clearTimeout(timer));
