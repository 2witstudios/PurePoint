// Reuse the merged bridge, with an ephemeral listener and deterministic RPC child.
// No native sessions, owner auth, pairing pages, or global installs are touched.
import { pathToFileURL } from 'node:url';
import path from 'node:path';
import { writeFile } from 'node:fs/promises';
import { randomUUID } from 'node:crypto';
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
const directory = process.env.CFFIXED_USER_HOME;
if (!directory?.startsWith('/tmp/pointguard-checks-')) throw new Error('Isolated fixture home required');
const { openTrustStore } = await load('trust.js');
const trust = await openTrustStore({directory: path.join(directory, 'trust')});
const desktopClientId = randomUUID();
const local = await serve(controller, {host:'127.0.0.1',port:0,localAdmin:{token:process.env.POINTGUARD_FIXTURE_TOKEN,clientId:desktopClientId}});
const remote = await serve(controller, {host:'127.0.0.1',port:0,trust,tls:trust.tls});
const phoneEndpoint = `wss://127.0.0.1:${remote.address().port}/v1`;
const enrollment = trust.createEnrollment({endpoint:phoneEndpoint});
const device = await trust.enroll({enrollmentToken:JSON.parse(enrollment.payload).enrollmentToken,name:'Fixture phone'});
await writeFile(path.join(directory,'phone-trust.json'),JSON.stringify({...device,endpoint:phoneEndpoint,certificateSHA256:trust.certificateSHA256}),{mode:0o600});
console.log(JSON.stringify({port:local.address().port,desktopClientId}));
const close = async () => { controller.dispose(); await local.shutdown(); await remote.shutdown(); await rpc.close(); await trust.close(); process.exit(0); };
process.on('SIGTERM', close);
process.on('SIGINT', close);
