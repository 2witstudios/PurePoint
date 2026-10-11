import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, mkdir, cp, writeFile, readFile, rm } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { once } from "node:events";
import { Rpc } from "./rpc.js";
import { root, launchArguments } from "./setup.js";

const provider = `
import { createAssistantMessageEventStream, fauxAssistantMessage, fauxToolCall, getCurrentSystemPrompt, getCurrentTools } from "@earendil-works/pi-ai";
import { appendFile } from "node:fs/promises";
export default function(pi) {
 pi.registerProvider("pointguard-proof", {
  baseUrl:"http://fixture.invalid", api:"pointguard-proof", apiKey:"fixture-only",
  models:[{id:"proof",name:"proof",reasoning:false,input:["text"],cost:{input:0,output:0,cacheRead:0,cacheWrite:0},contextWindow:200000,maxTokens:1024}],
  streamSimple(model, context, options) {
   const stream=createAssistantMessageEventStream();
   void (async()=>{
    const prompt=getCurrentSystemPrompt(context.messages);
    await appendFile(process.env.PROOF_REPORTS, JSON.stringify({prompt,tools:getCurrentTools(context.messages).map(t=>t.name)})+"\\n");
    const last=context.messages.at(-1);
    const text=typeof last?.content==="string"?last.content:(last?.content??[]).map(b=>b.text??"").join("");
    if(text==="slow request") {
     await new Promise(resolve=>{ if(options?.signal?.aborted) resolve(); else options?.signal?.addEventListener("abort",resolve,{once:true}); });
     const m={...fauxAssistantMessage("",{stopReason:"aborted"}),api:model.api,provider:model.provider,model:model.id};
     stream.push({type:"error",reason:"aborted",error:m});stream.end(m);return;
    }
    let content="fixture response";
    if(last?.role!=="toolResult" && text==="make artifact") content=fauxToolCall("write",{path:"artifact.txt",content:"verified artifact"});
    if(last?.role!=="toolResult" && text==="storage evidence") content=fauxToolCall("bash",{command:"df -k ."});
    const m={...fauxAssistantMessage(content,{stopReason:typeof content==="string"?"stop":"toolUse"}),api:model.api,provider:model.provider,model:model.id};
    stream.push({type:"start",partial:m});stream.push({type:"done",reason:m.stopReason,message:m});stream.end(m);
   })().catch(e=>{ const m=fauxAssistantMessage("",{stopReason:"error",errorMessage:String(e)});stream.push({type:"error",reason:"error",error:m});stream.end(m); });
   return stream;
  }
 });
 for(const method of ["confirm","select","input","editor"]) pi.registerCommand("proof-"+method,{description:"fixture dialog",handler:async(_args,ctx)=>{
  const value=method==="confirm"?await ctx.ui.confirm("proof","confirm"):
   method==="select"?await ctx.ui.select("proof",["one","two"]):
   method==="input"?await ctx.ui.input("proof"):await ctx.ui.editor("proof","draft");
  pi.sendMessage({customType:"proof-dialog",content:JSON.stringify({method,value}),display:true},{triggerTurn:false});
 }});
 pi.on("session_before_compact",(event)=>({compaction:{summary:"Fixture compaction preserves requested context.",firstKeptEntryId:event.preparation.firstKeptEntryId,tokensBefore:event.preparation.tokensBefore}}));
}
`;

test(
  "exact Pi 1.1.0 relocated RPC loads packages, refreshes sections across reload/resume/compaction and preserves native tools/UI/cancel",
  { timeout: 120000 },
  async (t) => {
    const folder = await mkdtemp(
      path.join(os.tmpdir(), "pointguard-rpc-proof-"),
    );
    let rpc;
    t.after(async () => {
      await rpc?.close();
      await rm(folder, { recursive: true, force: true });
    });
    const relocated = path.join(folder, "relocated");
    await mkdir(relocated);
    for (const name of [
      "node_modules",
      "support",
      "docs",
      "bridge",
      "package.json",
    ])
      await cp(path.join(root, name), path.join(relocated, name), {
        recursive: true,
      });
    const home = path.join(folder, "home"),
      cwd = path.join(folder, "cwd"),
      personal = path.join(home, "managed/personal"),
      reports = path.join(folder, "reports.jsonl");
    await mkdir(home, { mode: 0o700 });
    await mkdir(cwd, { mode: 0o700 });
    await mkdir(path.join(home, ".pi/agent"), { recursive: true, mode: 0o700 });
    await writeFile(
      path.join(home, ".pi/agent/settings.json"),
      JSON.stringify({
        compaction: { keepRecentTokens: 20, reserveTokens: 1024 },
      }),
    );
    const fixture = path.join(relocated, "proof-provider.ts");
    await writeFile(fixture, provider);
    const originalArgs = await launchArguments(cwd, {});
    const args = originalArgs.map((x) =>
      x.startsWith(root) ? path.join(relocated, path.relative(root, x)) : x,
    );
    args.push(
      "--extension",
      fixture,
      "--provider",
      "pointguard-proof",
      "--model",
      "proof",
      "--thinking",
      "off",
      "--offline",
    );
    const events = [];
    const launch = () => {
      const child = new Rpc(process.execPath, args, cwd, {
        HOME: home,
        PATH: "/usr/bin:/bin",
        PI_CODING_AGENT_DIR: path.join(home, ".pi/agent"),
        PI_SKIP_VERSION_CHECK: "1",
        PI_OFFLINE: "1",
        POINT_GUARD_PERSONAL_DIR: personal,
        PROOF_REPORTS: reports,
        PU_AGENT_ID: "must-not-inherit",
        PU_PROJECT_ROOT: "must-not-inherit",
      });
      child.on("failure", () => {});
      child.on("event", (e) => events.push(e));
      return child;
    };
    const prompt = async (message) => {
      const settled = once(rpc, "event");
      await rpc.call("prompt", { message });
      await settled;
      await waitFor(() => events.some((e) => e.type === "agent_settled"));
    };
    const run = async (message) => {
      events.length = 0;
      await prompt(message);
    };
    const report = async () =>
      JSON.parse((await readFile(reports, "utf8")).trim().split("\n").at(-1));
    rpc = launch();
    let state = await rpc.call("get_state");
    assert.equal(state.model.provider, "pointguard-proof");
    assert.deepEqual((await rpc.call("get_messages")).messages, []);
    const commands = await rpc.call("get_commands");
    assert.ok(commands.commands.some((c) => c.name === "pointguard"));
    await rpc.call("prompt", { message: "/pointguard info" });
    const messages = await rpc.call("get_messages");
    const info = messages.messages.find(
      (m) => m.customType === "pointguard-info",
    );
    assert.ok(info);
    const details = JSON.parse(info.content);
    assert.equal(
      details.workflowPackage.provenance.commit,
      "a12cecee28e8c22c250da6c30aecbdcad2f38fe5",
    );
    assert.equal(details.dependencies.pagespace, null);
    assert.equal(details.skills.length, 17);
    assert.deepEqual(details.activeTools, ["read", "bash", "edit", "write"]);
    // Native temporary package precedence wins; diagnostics identify the user loser.
    const userTask = path.join(home, ".pi/agent/skills/task/SKILL.md");
    await mkdir(path.dirname(userTask), { recursive: true });
    await writeFile(
      userTask,
      "---\nname: task\ndescription: Explicit user override.\n---\nUser task workflow.\n",
    );
    await rpc.call("prompt", { message: "/pointguard reload" });
    await rpc.call("prompt", { message: "/pointguard info" });
    const reloadedMessages = await rpc.call("get_messages");
    const latestInfo = JSON.parse(
      reloadedMessages.messages
        .filter((m) => m.customType === "pointguard-info")
        .at(-1).content,
    );
    assert.equal(latestInfo.skills.length, 17);
    assert.equal(
      latestInfo.skills.find((s) => s.name === "task").filePath,
      path.join(relocated, "support/pointguard-workflows/skills/task/SKILL.md"),
    );
    const collisions = latestInfo.nativeManagedDiagnostics.filter(
      (d) => d.type === "collision",
    );
    assert.equal(collisions.length, 1);
    assert.equal(collisions[0].collision.loserPath, userTask);
    for (const method of ["confirm", "select", "input", "editor"]) {
      events.length = 0;
      const pending = rpc.call("prompt", { message: "/proof-" + method });
      await waitFor(() =>
        events.some(
          (e) => e.type === "extension_ui_request" && e.method === method,
        ),
      );
      const request = events.find(
        (e) => e.type === "extension_ui_request" && e.method === method,
      );
      rpc.answer({
        id: request.id,
        ...(method === "confirm"
          ? { confirmed: true }
          : { value: method === "select" ? "one" : "proof text" }),
      });
      await pending;
    }
    await run("make artifact");
    assert.equal(
      await readFile(path.join(cwd, "artifact.txt"), "utf8"),
      "verified artifact",
    );
    await run("storage evidence");
    assert.ok(
      events.some(
        (e) =>
          e.type === "tool_execution_end" &&
          e.toolName === "bash" &&
          !e.isError,
      ),
    );
    const first = await report();
    assert.deepEqual(first.tools, ["read", "bash", "edit", "write"]);
    for (const tag of [
      "pointguard_soul",
      "pointguard_user",
      "pointguard_operating",
      "available_skills",
    ]) {
      assert.equal(first.prompt.split("<" + tag + ">").length - 1, 1, tag);
    }
    await writeFile(path.join(personal, "SOUL.md"), "CURRENT SOUL");
    await writeFile(path.join(personal, "USER.md"), "CURRENT PREFERENCE");
    await rpc.call("prompt", { message: "/pointguard reload" });
    await run("after reload");
    assert.match((await report()).prompt, /CURRENT SOUL/);
    state = await rpc.call("get_state");
    const saved = state.sessionFile;
    assert.ok(saved);
    await rpc.call("new_session");
    // Diagnostics belong to the old conversation; starting a chat must be empty.
    assert.deepEqual((await rpc.call("get_messages")).messages, []);
    await run("new session");
    assert.match((await report()).prompt, /CURRENT PREFERENCE/);
    await rpc.call("switch_session", { sessionPath: saved });
    await run("resumed session");
    await rpc.call("compact");
    await writeFile(path.join(personal, "SOUL.md"), "AFTER COMPACTION");
    await run("compacted session");
    const compacted = await report();
    assert.match(compacted.prompt, /AFTER COMPACTION/);
    assert.doesNotMatch(compacted.prompt, /CURRENT SOUL/);
    assert.equal(compacted.prompt.split("<pointguard_soul>").length - 1, 1);
    events.length = 0;
    await rpc.call("prompt", { message: "slow request" });
    await waitFor(() => events.some((e) => e.type === "agent_start"));
    await rpc.call("abort");
    await waitFor(() => events.some((e) => e.type === "agent_settled"));
    await rpc.close();
    assert.notEqual(rpc.child.exitCode, null);
    rpc = null;
    const before = await readFile(path.join(personal, "SOUL.md"), "utf8");
    rpc = launch();
    await rpc.call("get_state");
    assert.equal(
      await readFile(path.join(personal, "SOUL.md"), "utf8"),
      before,
    );
    assert.ok(
      !events.some((e) => e.type === "extension_error"),
      JSON.stringify(events.filter((e) => e.type === "extension_error")),
    );
  },
);
async function waitFor(check) {
  for (let i = 0; i < 600; i++) {
    if (check()) return;
    await new Promise((r) => setTimeout(r, 20));
  }
  throw new Error("RPC proof event timeout");
}
