// omp extension for pipeline sessions (decision 56), loaded with `-e` on each launch. omp is a
// pi fork, but it asks for tool approval and has an `ask` tool and plan reviews, which wait for a
// human. In a planning session it also guards edits, marks the record working and records the
// session file for Resume (omp picks the id itself), like Claude's planning hooks. In an item
// session it offers the workitem MCP server's tools. Outside a pipeline session it does nothing.
import { registerMcpTools } from "../mcp.ts";
import { idleReporter, notify, planNote, refusedEdit, resume } from "../notify.ts";

export default async function (pi) {
  const reporter = idleReporter(1000);
  pi.on("session_start", (_event, ctx) => {
    const file = ctx?.sessionManager?.getSessionFile?.();
    if (file) return planNote("--agent-session", file);
  });
  pi.on("agent_start", () => {
    reporter.busy();
    return Promise.all([resume(), planNote("--status", "working")]);
  });
  // An approved tool ran: the agent goes on.
  pi.on("tool_result", () => resume());
  pi.on("agent_end", () => reporter.idle());
  pi.on("tool_approval_requested", () => notify("permission_prompt"));
  pi.on("plan_review_requested", () => notify("elicitation_dialog"));
  pi.on("tool_call", async (event, ctx) => {
    if (event?.toolName === "ask") notify("elicitation_dialog");
    const reason = await refusedEdit(event?.toolName, event?.input, ctx?.cwd ?? process.cwd());
    if (reason) return { block: true, reason };
  });
  await registerMcpTools(pi);
}
