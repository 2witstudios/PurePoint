// Reuse the merged bridge, with an ephemeral listener and deterministic RPC child.
// No native sessions, owner auth, pairing pages, or global installs are touched.
import { pathToFileURL } from 'node:url';
import path from 'node:path';
const root = path.resolve(process.argv[2]);
const load = (name) => import(pathToFileURL(path.join(root, 'bridge', name)));
const { Rpc } = await load('rpc.js');
const { Controller } = await load('controller.js');
const { serve } = await load('network.js');
const rpc = new Rpc(process.execPath, [path.join(root, 'bridge/fixture.js')], root, process.env);
const controller = new Controller(rpc, {
  list: async () => [{ id: 'fixture-history', name: 'Previous conversation', modified: new Date() }],
  path: async () => 'fixture-history',
  history: async () => ({ sessionId: 'fixture-history', title: 'Previous conversation', messages: [{ id: 'old', role: 'assistant', text: 'History is read-only.' }] }),
});
await controller.refresh();
const server = await serve(controller, { host: '127.0.0.1', port: 0, token: process.env.POINTGUARD_FIXTURE_TOKEN });
console.log(server.address().port);
const close = async () => { controller.dispose(); await server.shutdown(); await rpc.close(); process.exit(0); };
process.on('SIGTERM', close);
process.on('SIGINT', close);
