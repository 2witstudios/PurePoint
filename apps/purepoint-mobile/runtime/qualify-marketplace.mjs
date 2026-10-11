import { mkdtemp, mkdir, writeFile, readFile, rm } from "node:fs/promises";
import os from "node:os";
import {
  catalogResult,
  confirmExit,
  qualificationFailed,
} from "./qualification-result.mjs";
import path from "node:path";
const mobile = path.resolve(process.argv[2]);
const artifacts = path.resolve(process.argv[3]);
const resultFile = path.resolve(process.argv[4]);
const { Rpc } = await import(path.join(mobile, "bridge/rpc.js"));
const { DefaultPackageManager } = await import(
  path.join(
    mobile,
    "node_modules/@earendil-works/pi-coding-agent/dist/core/package-manager.js",
  )
);
const { SettingsManager } = await import(
  path.join(
    mobile,
    "node_modules/@earendil-works/pi-coding-agent/dist/core/settings-manager.js",
  )
);
const scratch = await mkdtemp(
  path.join(os.tmpdir(), "pointguard-market-proof-"),
);
const output = [];
try {
  const fixture = path.join(scratch, "proof.ts");
  await writeFile(
    fixture,
    `import { createAssistantMessageEventStream,fauxAssistantMessage } from '@earendil-works/pi-ai';
 export default function(pi){pi.registerProvider('qualification',{baseUrl:'http://fixture.invalid',api:'qualification',apiKey:'fixture-only',models:[{id:'proof',name:'proof',reasoning:false,input:['text'],cost:{input:0,output:0,cacheRead:0,cacheWrite:0},contextWindow:200000,maxTokens:512}],streamSimple(model){const s=createAssistantMessageEventStream();const m={...fauxAssistantMessage('qualification'),api:model.api,provider:model.provider,model:model.id};s.push({type:'start',partial:m});s.push({type:'done',reason:'stop',message:m});s.end(m);return s;}});
 pi.registerCommand('qualification-info',{description:'fixture diagnostics',handler:(_,ctx)=>pi.sendMessage({customType:'qualification',content:JSON.stringify({tools:pi.getAllTools().map(t=>({name:t.name,exposure:t.exposure})),active:pi.getActiveTools(),skills:ctx.getSystemPromptOptions().skills.map(s=>s.name)}),display:true},{triggerTurn:false})});}`,
  );
  for (const name of [
    "pi-web-access",
    "pi-playwright",
    "pi-agent-browser-native",
    "@amaster.ai-pi-computer-use",
  ]) {
    const pkg = path.join(artifacts, name, "package");
    const manifest = JSON.parse(
      await readFile(path.join(pkg, "package.json"), "utf8"),
    );
    const home = path.join(scratch, name.replaceAll("/", "-"));
    await mkdir(home, { mode: 0o700 });
    const manager = new DefaultPackageManager({
      cwd: home,
      agentDir: path.join(home, ".pi/agent"),
      settingsManager: SettingsManager.inMemory(),
    });
    const resolved = await manager.resolveExtensionSources([pkg], {
      temporary: true,
    });
    const record = {
      name: manifest.name,
      version: manifest.version,
      node: process.version,
      pi: "1.1.0",
      engines: manifest.engines ?? null,
      extensions: resolved.extensions.map((e) => path.relative(pkg, e.path)),
      skills: resolved.skills.map((s) => path.relative(pkg, s.path)),
    };
    if (name === "pi-agent-browser-native") {
      record.result =
        "REJECT: published Node >=24.21.0 exceeds pinned Node22.23.3; not executed";
      output.push(record);
      continue;
    }
    const rpc = new Rpc(
      process.execPath,
      [
        path.join(mobile, "bridge/native.js"),
        "--mode",
        "rpc",
        "--extension",
        pkg,
        "--extension",
        fixture,
        "--offline",
        "--no-session",
        "--provider",
        "qualification",
        "--model",
        "proof",
      ],
      home,
      {
        HOME: home,
        PATH: "/usr/bin:/bin",
        PI_CODING_AGENT_DIR: path.join(home, ".pi/agent"),
        PI_SKIP_VERSION_CHECK: "1",
      },
    );
    const events = [];
    let stderr = "";
    rpc.child.stderr.on("data", (b) => (stderr += b));
    rpc.on("failure", () => {});
    rpc.on("event", (e) => events.push(e));
    try {
      await rpc.call("get_state", {}, 20000);
      await rpc.call("prompt", { message: "/qualification-info" }, 20000);
      const m = await rpc.call("get_messages");
      record.catalog = JSON.parse(
        m.messages.find((m) => m.customType === "qualification").content,
      );
      record.extensionErrors = events.filter(
        (e) => e.type === "extension_error",
      );
      record.result = catalogResult(record);
    } catch (e) {
      record.result = "FAIL native RPC startup: " + e.message;
      record.stderr = stderr.slice(0, 3000);
    } finally {
      await rpc.close();
      record.teardown = await confirmExit(rpc.child);
      if (!record.teardown) {
        record.result = "FAIL unconfirmed child teardown; " + record.result;
        // This is our disposable child only. Reap it even when qualification fails.
        rpc.child.kill("SIGKILL");
        await confirmExit(rpc.child);
      }
    }
    output.push(record);
  }
  await writeFile(resultFile, JSON.stringify(output, null, 2) + "\n");
  console.log(JSON.stringify(output, null, 2));
  if (qualificationFailed(output)) process.exitCode = 1;
} finally {
  await rm(scratch, { recursive: true, force: true });
}
