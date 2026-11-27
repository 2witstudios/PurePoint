// Round-trips {"type":"health"} over a Windows named pipe: node health.mjs <pipe>
import net from 'node:net';

const path = process.argv[2];
const timer = setTimeout(() => { console.error('timeout'); process.exit(3); }, 5000);
const sock = net.createConnection(path);
let buf = '';
sock.setEncoding('utf8');
sock.on('connect', () => sock.write(JSON.stringify({ type: 'health' }) + '\n'));
sock.on('data', (chunk) => {
  buf += chunk;
  const nl = buf.indexOf('\n');
  if (nl < 0) return;
  const msg = JSON.parse(buf.slice(0, nl));
  console.log(JSON.stringify(msg));
  process.exitCode = msg.type === 'health_report' ? 0 : 2;
  clearTimeout(timer);
  sock.end();
});
sock.on('error', (e) => { console.error(e.message); process.exitCode = 1; clearTimeout(timer); });
