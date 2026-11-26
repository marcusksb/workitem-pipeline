// opencode plugin for pipeline sessions (decision 56). The launcher loads it through
// OPENCODE_CONFIG_CONTENT; outside a workitem or planning session it does nothing.
// It reports waiting to bin/workitem-notify and, in a planning session, does what Claude's
// planning hooks do: guards edits, marks the record working, and records the session for Resume.
// The workitem MCP server comes through the config, not through the plugin.
import { notify, planNote, refusedEdit, resume } from "../notify.ts";

export const WorkitemNotify = async ({ client, directory }) => {
  // A subagent's session has a parent; its events are not the main session's.
  const isSubagent = async (id) => {
    try {
      const session = await client?.session?.get({ path: { id } });
      return Boolean(session?.data?.parentID);
    } catch {
      return false;
    }
  };
  return {
    event: async ({ event }) => {
      const props = event.properties ?? {};
      if (event.type === "permission.asked") return notify("permission_prompt");
      if (event.type === "question.asked") return notify("elicitation_dialog");
      if (event.type === "session.created" && props.info?.id && !props.info.parentID) return planNote("--agent-session", props.info.id);
      if (event.type === "permission.replied") return resume();
      if (event.type === "session.status" && props.status?.type === "busy") return Promise.all([resume(), planNote("--status", "working")]);
      if (event.type === "session.idle" && !(await isSubagent(props.sessionID))) return notify("idle_prompt");
    },
    "tool.execute.before": async (input, output) => {
      const reason = await refusedEdit(input?.tool, output?.args, directory);
      if (reason) throw new Error(reason);
    },
  };
};
