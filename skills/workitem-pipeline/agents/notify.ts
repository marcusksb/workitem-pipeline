// Shared by the agents' hook glue (decision 56): reports to bin/workitem-notify that the session
// waits for input, the way Claude Code's Notification hook does. The blocked and waiting rules
// stay in that script. Plain JavaScript in a .ts file, so every runtime (bun, jiti, node) loads it.
import { spawn } from "node:child_process";
import { fileURLToPath } from "node:url";

const bin = (name) => fileURLToPath(new URL(`../bin/${name}`, import.meta.url));

/** The script runs so far; each new one starts after them. */
let queue = Promise.resolve();

/**
 * Runs a pipeline script with `input` on stdin; resolves to its exit code and stderr. Runs one
 * at a time, in call order: the scripts rewrite the same file, and a "working" note that finished
 * after a later "waiting" report would undo it (opencode reports busy several times per turn).
 */
function run(name, args, input) {
  const next = queue.then(() => spawnScript(name, args, input));
  queue = next.catch(() => {});
  return next;
}

function spawnScript(name, args, input) {
  return new Promise((resolve) => {
    const child = spawn("python3", [bin(name), ...args], { stdio: ["pipe", "ignore", "pipe"] });
    let stderr = "";
    child.stderr.on("data", (chunk) => (stderr += chunk));
    child.on("error", () => resolve({ code: -1, stderr }));
    child.on("close", (code) => resolve({ code, stderr }));
    child.stdin.on("error", () => {});
    child.stdin.end(input);
  });
}

/**
 * `type` is Claude's notification_type: "idle_prompt" when the agent stopped and waits for the
 * next message, "permission_prompt" or "elicitation_dialog" when a human must answer a dialog.
 * Does nothing outside a workitem, planning or create session.
 */
export async function notify(type) {
  if (!process.env.WORKITEM && !process.env.WORKITEM_PLAN && !process.env.WORKITEM_CREATE) return;
  await run("workitem-notify", [], JSON.stringify({ notification_type: type }));
}

/**
 * The agent goes on: a create session's toast stops showing it waiting, as Claude's
 * UserPromptSubmit and PostToolUse hooks do (decision 75). Nothing in other sessions.
 */
export async function resume() {
  if (!process.env.WORKITEM_CREATE) return;
  await run("workitem-notify", ["--resume"], "");
}

/** Updates the planning session's record (bin/workitem-plan-note); nothing outside one. */
export async function planNote(...args) {
  if (!process.env.WORKITEM_PLAN) return;
  await run("workitem-plan-note", args, "");
}

/**
 * Claude's PreToolUse guard of planning sessions (bin/workitem-plan-guard): resolves to why a
 * file edit is refused, or null. Only tools that write files are checked; reading stays free.
 */
export async function refusedEdit(tool, input, cwd) {
  if (!process.env.WORKITEM_PLAN || !/edit|write|patch/i.test(tool ?? "")) return null;
  const targets = [input?.file_path ?? input?.filePath ?? input?.path, ...patchedFiles(input?.patchText ?? input?.patch ?? input?.input)].filter(Boolean);
  for (const target of targets) {
    const { code, stderr } = await run("workitem-plan-guard", [], JSON.stringify({ tool_name: tool, cwd, tool_input: { file_path: target } }));
    if (code === 2) return stderr.trim();
  }
  return null;
}

/** The files a patch (`*** Update File: <path>` and the like) adds, changes, moves or deletes. */
function patchedFiles(patch) {
  if (typeof patch !== "string") return [];
  return [...patch.matchAll(/^\*\*\* (?:Add File|Update File|Delete File|Move to): (.+)$/gm)].map((m) => m[1].trim());
}

/**
 * Reports idle only when the agent stays stopped for `ms`: a subagent's loop can end while the
 * main one goes on, and `busy` (on the next start) cancels the report.
 */
export function idleReporter(ms) {
  let timer;
  return {
    idle() {
      clearTimeout(timer);
      timer = setTimeout(() => notify("idle_prompt"), ms);
      timer.unref?.();
    },
    busy() {
      clearTimeout(timer);
    },
  };
}
