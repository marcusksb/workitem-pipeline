// Feeds each agent's hook glue fake events and checks that the real bin/workitem-notify marks the
// workitem the way Claude's Notification hook would. Run: node --test skills/workitem-pipeline/agents/
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

import { WorkitemNotify } from "./opencode/workitem-notify.ts";
import pi from "./pi/workitem-notify.ts";
import omp from "./omp/workitem-notify.ts";

// The workspace holds the items and plans; the repo holds code, which planning may not edit.
const root = mkdtempSync(join(tmpdir(), "glue-"));
const ws = join(root, "ws");
const repo = join(root, "repo");
const file = join(ws, "T-1", "workitem.md");
mkdirSync(join(ws, "T-1"), { recursive: true });
mkdirSync(repo);
process.env.WORKITEM = "T-1";
process.env.WORKITEM_FILE = file;
process.env.WORKITEM_WORKSPACE = ws;

const setStatus = (status) => writeFileSync(file, `---\nstage: analyze\nagent_status: ${status}\n---\n`);
const status = () => readFileSync(file, "utf8").match(/^agent_status: (.*)$/m)[1];
const blocked = "blocked: waiting for input, tmux attach -t wi-T-1";

/** Loads a pi or omp extension and returns its handlers by event; `tools` gets its tools. */
function extension(factory, tools = {}) {
  const handlers = {};
  const loaded = factory({ on: (name, handler) => (handlers[name] ??= []).push(handler), registerTool: (tool) => (tools[tool.name] = tool) });
  const fire = (name, event = {}) => Promise.all((handlers[name] ?? []).map((h) => h(event, {})));
  fire.loaded = loaded;
  return fire;
}

async function eventually(what, check) {
  for (let i = 0; i < 50; i++) {
    if (check()) return;
    await sleep(100);
  }
  assert.fail(what);
}

test("opencode: idle main session, permission and question block; a subagent's idle does not", async () => {
  const sessions = { main: {}, sub: { parentID: "main" } };
  const client = { session: { get: async ({ path }) => ({ data: sessions[path.id] }) } };
  const plugin = await WorkitemNotify({ client });
  const fire = (type, properties = {}) => plugin.event({ event: { type, properties } });

  setStatus("working");
  await fire("session.idle", { sessionID: "sub" });
  await fire("session.status", { sessionID: "main" });
  assert.equal(status(), "working");
  await fire("session.idle", { sessionID: "main" });
  assert.equal(status(), blocked);

  setStatus("waiting: CI");
  await fire("session.idle", { sessionID: "main" });
  assert.equal(status(), "waiting: CI", "a waiting agent idles on purpose");
  await fire("permission.asked");
  assert.equal(status(), blocked, "a permission prompt needs a human");

  setStatus("waiting: CI");
  await fire("question.asked");
  assert.equal(status(), blocked, "a question needs a human");
});

test("pi: a stopped agent blocks unless it starts again at once", async () => {
  const fire = extension(pi);
  setStatus("working");
  await fire("agent_end");
  await fire("agent_start");
  await sleep(1300);
  assert.equal(status(), "working", "a loop that ends and goes on is not idle");
  await fire("agent_end");
  await eventually("idle blocks", () => status() === blocked);
});

test("omp: approval, ask and plan review block a waiting agent; idle blocks a working one", async () => {
  const fire = extension(omp);
  for (const [name, event] of [["tool_approval_requested"], ["plan_review_requested"], ["tool_call", { toolName: "ask" }]]) {
    setStatus("waiting: CI");
    await fire(name, event);
    await eventually(`${name} blocks`, () => status() === blocked);
  }
  setStatus("working");
  await fire("tool_call", { toolName: "bash" });
  await fire("agent_end");
  await eventually("idle blocks", () => status() === blocked);
});

/** Runs `body` as a planning session with a fresh record; the workitem env is unset meanwhile. */
async function planning(body) {
  const record = join(ws, "plans", "p1.md");
  mkdirSync(join(ws, "plans"), { recursive: true });
  writeFileSync(record, "---\ntopic: t\nstatus: waiting\nagent_session: \n---\n\n## Prompt\n\nt\n");
  const saved = process.env.WORKITEM;
  delete process.env.WORKITEM;
  process.env.WORKITEM_PLAN = record;
  try {
    await body(() => readFileSync(record, "utf8"));
  } finally {
    process.env.WORKITEM = saved;
    delete process.env.WORKITEM_PLAN;
  }
}

test("opencode planning: records its session, marks working, refuses code edits", () =>
  planning(async (text) => {
    const plugin = await WorkitemNotify({ client: { session: { get: async () => ({ data: {} }) } }, directory: repo });
    const fire = (type, properties) => plugin.event({ event: { type, properties } });
    await fire("session.created", { info: { id: "ses_sub", parentID: "ses_main" } });
    await fire("session.created", { info: { id: "ses_main" } });
    assert.match(text(), /^agent_session: ses_main$/m, "a subagent's session is not the plan's");
    await fire("session.status", { sessionID: "ses_main", status: { type: "busy" } });
    assert.match(text(), /^status: working$/m);
    await fire("session.idle", { sessionID: "ses_main" });
    assert.match(text(), /^status: waiting$/m);

    const edit = (tool, args) => plugin["tool.execute.before"]({ tool }, { args });
    await assert.rejects(edit("edit", { filePath: join(repo, "app.py") }), /planning session/);
    await edit("write", { filePath: join(ws, "X-1", "workitem.md") });
    await edit("read", { filePath: join(repo, "app.py") });
  }));

test("pi and omp planning: mark working, refuse code edits; omp records its session file", () =>
  planning(async (text) => {
    for (const factory of [pi, omp]) {
      const fire = extension(factory);
      await fire("agent_start");
      assert.match(text(), /^status: working$/m);
      const [blocked] = await fire("tool_call", { toolName: "edit", input: { path: join(repo, "src", "a.ts") } });
      assert.equal(blocked?.block, true);
      assert.match(blocked.reason, /planning session/);
      const [allowed] = await fire("tool_call", { toolName: "write", input: { path: join(ws, "plans", "x.md") } });
      assert.equal(allowed, undefined);
      const [read] = await fire("tool_call", { toolName: "read", input: { path: join(repo, "src", "a.ts") } });
      assert.equal(read, undefined);
    }
    const handlers = {};
    omp({ on: (name, handler) => (handlers[name] = handler) });
    await handlers.session_start({}, { sessionManager: { getSessionFile: () => "/s/2026_abc.jsonl" } });
    assert.match(text(), /^agent_session: \/s\/2026_abc\.jsonl$/m);
  }));

test("create sessions: waiting sets the toast's option, going on unsets it", async () => {
  // A fake tmux on PATH records what workitem-notify asks of it.
  const bin = mkdtempSync(join(tmpdir(), "tmux-"));
  const calls = join(bin, "calls");
  writeFileSync(join(bin, "tmux"), `#!/bin/sh\necho "$@" >> ${calls}\n`, { mode: 0o755 });
  const saved = { PATH: process.env.PATH, WORKITEM: process.env.WORKITEM };
  process.env.PATH = `${bin}:${saved.PATH}`;
  process.env.WORKITEM_CREATE = "wi-new-1";
  delete process.env.WORKITEM;
  const lines = () => readFileSync(calls, "utf8").trim().split("\n");
  try {
    const plugin = await WorkitemNotify({ client: { session: { get: async () => ({ data: {} }) } } });
    const fire = (type, properties = {}) => plugin.event({ event: { type, properties } });
    await fire("permission.asked");
    assert.deepEqual(lines(), ["set-option -t =wi-new-1: @workitem_waiting 1"]);
    await fire("permission.replied");
    assert.equal(lines()[1], "set-option -u -t =wi-new-1: @workitem_waiting");
    for (const factory of [pi, omp]) {
      writeFileSync(calls, "");
      const fire = extension(factory);
      await fire("agent_end");
      await eventually("idle sets it", () => lines()[0] === "set-option -t =wi-new-1: @workitem_waiting 1");
      await fire("agent_start");
      assert.equal(lines()[1], "set-option -u -t =wi-new-1: @workitem_waiting");
    }
  } finally {
    process.env.PATH = saved.PATH;
    process.env.WORKITEM = saved.WORKITEM;
    delete process.env.WORKITEM_CREATE;
  }
});

test("pi and omp offer the workitem MCP server's tools", async () => {
  const requests = [];
  const server = createServer((req, res) => {
    let body = "";
    req.on("data", (chunk) => (body += chunk));
    req.on("end", () => {
      const message = JSON.parse(body);
      requests.push({ auth: req.headers.authorization, message });
      const result =
        message.method === "tools/list"
          ? { tools: [{ name: "ask_parent", description: "Ask the parent.", inputSchema: { type: "object", properties: { question: { type: "string" } } } }] }
          : { content: [{ type: "text", text: message.params.arguments.question === "fail" ? "no parent" : "yes" }], isError: message.params.arguments.question === "fail" };
      res.setHeader("content-type", "application/json");
      res.end(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }));
    });
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  const config = join(root, "mcp.json");
  const url = `http://127.0.0.1:${server.address().port}/api/mcp/app/T-1`;
  writeFileSync(config, JSON.stringify({ mcpServers: { workitem: { type: "http", url, headers: { Authorization: "Bearer tok" } } } }));
  process.env.WORKITEM_MCP = config;
  try {
    for (const factory of [pi, omp]) {
      const tools = {};
      await extension(factory, tools).loaded;
      assert.deepEqual(Object.keys(tools), ["ask_parent"]);
      assert.equal(tools.ask_parent.parameters.properties.question.type, "string", "the server's schema");
      const result = await tools.ask_parent.execute("call-1", { question: "which?" });
      assert.deepEqual(result.content, [{ type: "text", text: "yes" }]);
      await assert.rejects(tools.ask_parent.execute("call-2", { question: "fail" }), /no parent/, "a refusal is the tool's error");
    }
    assert.ok(requests.every((r) => r.auth === "Bearer tok"), "with the token");
    assert.deepEqual(requests[1].message.params, { name: "ask_parent", arguments: { question: "which?" } });
  } finally {
    delete process.env.WORKITEM_MCP;
    server.close();
  }
  const tools = {};
  await extension(pi, tools).loaded;
  assert.deepEqual(tools, {}, "no tools without WORKITEM_MCP");
});

test("outside a pipeline session nothing happens", async () => {
  const saved = process.env.WORKITEM;
  delete process.env.WORKITEM;
  try {
    setStatus("working");
    const plugin = await WorkitemNotify({});
    await plugin.event({ event: { type: "permission.asked", properties: {} } });
    assert.equal(status(), "working");
  } finally {
    process.env.WORKITEM = saved;
  }
});
