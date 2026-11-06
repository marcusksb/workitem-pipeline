// Runs the built backend against a real tmux session; needs `cargo build` and `npm run build` first.
import { expect, test } from "@playwright/test";
import { ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const port = 7392;
const socket = `wi-e2e-${process.pid}`;
let server: ChildProcess;
let base = "";
let repo = "";

test.beforeAll(async () => {
  const dir = mkdtempSync(join(tmpdir(), "wi-e2e-"));
  repo = join(dir, "repo");
  const item = join(repo, ".claude/workitems/T-1-demo");
  mkdirSync(item, { recursive: true });
  writeFileSync(
    join(item, "workitem.md"),
    "---\ntitle: Demo blocked item\ntype: bug\nstage: analyze\napproved: false\nagent_status: blocked: need your answer\nissue: T-1\npr: https://github.com/demo/repo/pull/42\n---\n\n# Demo\n\nHello brief text.\n",
  );
  const gate = join(repo, ".claude/workitems/T-2-gate");
  mkdirSync(gate, { recursive: true });
  writeFileSync(
    join(gate, "workitem.md"),
    "---\ntitle: Brief waiting\ntype: feature\nstage: ready_for_development\napproved: false\nagent_status: ok\nissue: T-2\n---\n\n# Gate\n",
  );
  mkdirSync(join(gate, "notes"), { recursive: true });
  writeFileSync(join(gate, "notes/findings.md"), "# Findings\n\n- the cache is stale\n");
  writeFileSync(join(gate, "log.txt"), "line one\nline two\n");
  const approve = join(repo, ".claude/workitems/T-3-approve");
  mkdirSync(approve, { recursive: true });
  writeFileSync(
    join(approve, "workitem.md"),
    "---\ntitle: Approve me\ntype: feature\nstage: ready_for_development\napproved: false\nagent_status: ok\nissue: T-3\n---\n\n# Approve\n",
  );
  mkdirSync(join(repo, ".claude/workitems/boards"), { recursive: true });
  writeFileSync(
    join(repo, ".claude/workitems/boards/research.json"),
    JSON.stringify({ brief_sections: [], entry: "idea", stages: [{ name: "idea", next: ["findings"] }, { name: "findings", next: [] }] }),
  );
  const research = join(repo, ".claude/workitems/T-4-research");
  mkdirSync(research, { recursive: true });
  writeFileSync(
    join(research, "workitem.md"),
    "---\ntitle: Research item\ntype: feature\nstage: idea\nboard: research\nagent_status: ok\nissue: T-4\n---\n\n# Research\n",
  );
  const tasks = join(repo, ".claude/workitems/T-5-tasks");
  mkdirSync(tasks, { recursive: true });
  const findings = Array.from({ length: 6 }, (_, i) => `- [ ] **Finding ${i}**\\\n  What is wrong and how to fix it.\n`).join("\n");
  writeFileSync(
    join(tasks, "workitem.md"),
    `---\ntitle: Task list\ntype: review\nstage: done\nagent_status: ok\nissue: T-5\n---\n\n# Review\n\n${"A paragraph.\n\n".repeat(30)}${findings}`,
  );
  const followUps = join(repo, ".claude/workitems/T-6-follow-ups");
  mkdirSync(followUps, { recursive: true });
  writeFileSync(
    join(followUps, "workitem.md"),
    `---\ntitle: Merged with follow-ups\ntype: feature\nstage: done\nagent_status: ok\nissue: T-6\n---\n\n# Done\n\n${"A paragraph.\n\n".repeat(40)}## Follow-ups\n\n- [ ] Deploy to prod\n- [x] Tell support\n- [ ] Remove the flag\n`,
  );
  writeFileSync(
    join(repo, ".claude/workitems/learnings.md"),
    "# Learnings\n\n- Never mock tmux in tests. (source: https://github.com/demo/repo/pull/42#discussion_r1)\n",
  );
  const plans = join(repo, ".claude/workitems/plans");
  mkdirSync(plans, { recursive: true });
  writeFileSync(
    join(plans, "abc123.md"),
    "---\ntopic: Split the auth module\nsummary: Comparing a facade with a full split\nstatus: waiting\nclaude_session: 1\ncreated: 2\n---\n\n## Prompt\n\nSplit the auth module\n\n## Plan\n\n1. Extract the token store\n",
  );
  writeFileSync(
    join(plans, "def456.md"),
    "---\ntopic: Old idea\nsummary: Parked\nstatus: ended\nclaude_session: 2\ncreated: 1\n---\n\n## Prompt\n\nOld idea\n",
  );
  execFileSync("tmux", ["-L", socket, "new-session", "-d", "-s", "wi-plan-abc123", "-c", repo, "echo planning-agent-ready; sleep 600"]);
  execFileSync("tmux", [
    "-L", socket, "new-session", "-d", "-s", "wi-T-1-demo", "-c", repo,
    "echo 'agent: which db do you want?'; read a; echo got:$a; sleep 600",
  ]);

  server = spawn(resolve("../backend/target/debug/workload"), [], {
    env: {
      ...process.env,
      WORKITEM_UI_CONFIG: join(dir, "config.toml"),
      WORKITEM_UI_PORT: String(port),
      WORKITEM_UI_TMUX_SOCKET: socket,
      WORKITEM_UI_DISPATCH: "off",
    },
  });
  base = await new Promise<string>((done, fail) => {
    server.stdout!.on("data", (chunk: Buffer) => {
      const url = /http:\/\/\S+/.exec(chunk.toString());
      if (url) done(url[0]);
    });
    server.on("error", fail);
  });
});

test.afterAll(() => {
  server?.kill();
  try {
    execFileSync("tmux", ["-L", socket, "kill-server"]);
  } catch {}
});

test.describe.configure({ mode: "serial" });

test("add a project, find the blocked item, answer the agent in the browser terminal", async ({ page }) => {
  await page.goto(base);
  await expect(page.getByText("No projects yet")).toBeVisible();
  expect(page.url()).not.toContain("token");

  await page.getByRole("button", { name: "Add project" }).click();
  await page.getByPlaceholder("name").fill("demo");
  await page.getByPlaceholder("/absolute/path/to/repo").fill(repo);
  await page.getByRole("button", { name: "Add", exact: true }).click();

  const board = page.locator(".board");
  await expect(board.getByText("Demo blocked item")).toBeVisible();
  await expect(board.locator(".card.blocked", { hasText: "Demo blocked item" })).toBeVisible();
  await board.getByText("Demo blocked item").click();

  const terminal = page.locator(".detail .xterm-rows");
  await expect(terminal).toContainText("which db do you want?");
  await page.locator(".detail .xterm").click();
  await page.keyboard.type("postgres\n");
  await expect(terminal).toContainText("got:postgres");

  await expect(page.getByRole("link", { name: "PR #42 ↗" }).first()).toHaveAttribute("href", "https://github.com/demo/repo/pull/42");
  await page.getByRole("button", { name: "PR", exact: true }).click();
  await expect(page.locator(".brief .pr-link")).toHaveAttribute("href", "https://github.com/demo/repo/pull/42");
  await page.getByRole("button", { name: "Brief", exact: true }).click();
  await expect(page.getByText("Hello brief text.")).toBeVisible();

  // Closing the panel detaches; the agent session survives.
  await page.getByRole("button", { name: "Close", exact: true }).click();
  execFileSync("tmux", ["-L", socket, "has-session", "-t", "wi-T-1-demo"]);
});

test("approve and reject at the gate from the Actions tab", async ({ page }) => {
  await page.goto(base);
  const gateColumn = page.locator(".column", { hasText: "ready for development" });

  // Approving moves the item on, to the stage the gate names.
  await gateColumn.getByText("Approve me").click();
  await page.getByRole("button", { name: "Actions" }).click();
  await page.getByRole("button", { name: "Approve", exact: true }).click();
  await expect(page.locator(".column", { hasText: "in development" })).toContainText("Approve me");
  await expect(gateColumn).not.toContainText("Approve me");

  // Rejecting needs a comment and sends the item to the stage the gate names.
  await gateColumn.getByText("Brief waiting").click();
  await page.getByRole("button", { name: "Actions" }).click();
  await page.getByPlaceholder("Why is this rejected?").fill("needs a rollback plan");
  await page.getByRole("button", { name: /Reject/ }).click();
  await expect(page.locator(".column", { hasText: "analyze" }).first()).toContainText("Brief waiting");
});

test("deep links and history navigate items and tabs", async ({ page }) => {
  await page.goto(base);
  await page.locator(".board").getByText("Demo blocked item").click();
  await expect(page).toHaveURL(/#demo\/T-1-demo$/);
  await page.getByRole("button", { name: "Actions" }).click();
  await expect(page).toHaveURL(/#demo\/T-1-demo\/actions$/);
  await expect(page.getByRole("button", { name: "Remove workitem" })).toBeVisible();

  await page.goBack();
  await expect(page).toHaveURL(/#demo\/T-1-demo$/);
  // T-1 is blocked with a live session, so the default tab is the terminal.
  await expect(page.locator(".detail .xterm-rows")).toBeVisible();
  await page.goForward();
  await expect(page).toHaveURL(/#demo\/T-1-demo\/actions$/);

  await page.getByRole("button", { name: "Close", exact: true }).click();
  await expect(page.locator(".detail")).toHaveCount(0);

  await page.goto(`${base.split("#")[0]}#demo/T-1-demo/terminal`);
  await expect(page.locator(".detail .xterm-rows")).toBeVisible();

  // A click on the board outside any card closes the item like the Close button.
  await page.locator(".column h2").first().click({ position: { x: 2, y: 2 } });
  await expect(page.locator(".detail")).toHaveCount(0);
});

test("a project's boards are tabs and the chosen board stays in the URL", async ({ page }) => {
  await page.goto(base);
  const tabs = page.getByRole("tablist");
  await expect(tabs.getByRole("tab", { name: "default" })).toHaveAttribute("aria-selected", "true");
  await expect(page.locator(".column", { hasText: "idea" })).toHaveCount(0);

  await tabs.getByRole("tab", { name: "research" }).click();
  await expect(page).toHaveURL(/#\?board=research$/);
  await expect(page.locator(".column", { hasText: "idea" })).toContainText("Research item");
  await expect(page.locator(".column", { hasText: "backlog" })).toHaveCount(0);

  // The choice survives a reload, and an item's deep link keeps it.
  await page.reload();
  await expect(tabs.getByRole("tab", { name: "research" })).toHaveAttribute("aria-selected", "true");
  await page.locator(".column", { hasText: "idea" }).getByText("Research item").click();
  await expect(page).toHaveURL(/#demo\/T-4-research\?board=research$/);
});

test("the selected project stays in the URL and survives a reload", async ({ page }) => {
  await page.goto(base);
  const select = page.locator(".projects select");
  await select.selectOption("demo");
  await expect(page).toHaveURL(/#\?project=demo$/);
  await page.reload();
  await expect(select).toHaveValue("demo");
  await select.selectOption("");
  await expect(page).not.toHaveURL(/project=/);
});

test("quick search opens an item by Jira key, PR number or title, on its own board", async ({ page }) => {
  await page.goto(base);
  const search = page.getByRole("combobox", { name: "Search workitems" });
  await expect(page.locator(".board").getByText("Demo blocked item")).toBeVisible();

  await page.keyboard.press("ControlOrMeta+k");
  await search.fill("t-4");
  await page.keyboard.press("Enter");
  await expect(page).toHaveURL(/#demo\/T-4-research\?board=research$/);
  await expect(search).toHaveCount(0);

  // The shortcut also works while the terminal has focus.
  await page.goto(`${base.split("#")[0]}#demo/T-1-demo/terminal`);
  await page.locator(".detail .xterm").click();
  await page.keyboard.press("ControlOrMeta+k");
  await search.fill("#42");
  await expect(page.getByRole("listbox").getByRole("option")).toHaveText([/Demo blocked item/]);
  await page.keyboard.press("Escape");
  await expect(search).toHaveCount(0);

  await page.getByRole("button", { name: /Search/ }).click();
  await search.fill("approve");
  await page.getByRole("option", { name: /Approve me/ }).click();
  await expect(page).toHaveURL(/#demo\/T-3-approve\?board=default$/);
});

test("the Files tab lists the workitem folder and previews a file", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Brief waiting");
  await page.goto(`${base.split("#")[0]}#demo/T-2-gate/files`);
  const list = page.locator(".file-list");
  await expect(list.getByRole("button")).toHaveText([/log\.txt/, /notes\/findings\.md/, /workitem\.md/]);
  await list.getByRole("button", { name: /log\.txt/ }).click();
  await expect(page.locator(".file-preview pre")).toHaveText("line one\nline two\n");
  await list.getByRole("button", { name: /findings\.md/ }).click();
  await expect(page.locator(".file-preview").getByRole("heading", { name: "Findings" })).toBeVisible();
  const open = page.locator(".file-preview").getByRole("link", { name: "Open" });
  await expect(page.getByRole("button", { name: "Show in Finder" })).toBeVisible();
  await expect(open).toHaveAttribute("href", "/api/workitems/demo/T-2-gate/files/notes/findings.md");
  await page.screenshot({ path: process.env.FILES_SHOT ?? join(tmpdir(), "files-tab.png") });
  const raw = await page.request.get(base.split("#")[0].replace(/\/$/, "") + "/api/workitems/demo/T-2-gate/files/notes/findings.md");
  expect(await raw.text()).toContain("the cache is stale");
});

test("the Learnings button shows and edits the learnings of the board on screen", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Brief waiting");
  await page.getByRole("button", { name: "Learnings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Learnings in demo" });
  await expect(dialog.getByRole("listitem")).toContainText("Never mock tmux in tests.");
  await expect(dialog.getByRole("link", { name: /pull\/42/ })).toHaveAttribute("target", "_blank");
  await expect(dialog.locator(".source-path code")).toContainText("learnings.md");
  await dialog.getByRole("button", { name: "Close" }).click();

  // Another board has its own file, empty until you add a rule.
  await page.getByRole("tablist").getByRole("tab", { name: "research" }).click();
  await page.getByRole("button", { name: "Learnings", exact: true }).click();
  const research = page.getByRole("dialog", { name: "Learnings in demo · research" });
  await expect(research.locator(".source-path code")).toContainText("boards/research/learnings.md");
  await expect(research).toContainText("No learnings yet");
  await research.getByRole("button", { name: "Edit" }).click();
  const editor = research.getByRole("textbox", { name: "Learnings" });
  await expect(editor).toHaveValue("# Learnings\n\n");
  await editor.fill("# Learnings\n\n- Flag only real bugs.\n");
  await research.getByRole("button", { name: "Save" }).click();
  await expect(research.getByRole("listitem")).toHaveText("Flag only real bugs.");
  expect(readFileSync(join(repo, ".claude/workitems/boards/research/learnings.md"), "utf8")).toBe("# Learnings\n\n- Flag only real bugs.\n");
  await page.screenshot({ path: process.env.LEARNINGS_SHOT ?? join(tmpdir(), "learnings.png") });
});

test("pausing a column marks it until it is resumed, and only watched columns can pause", async ({ page }) => {
  await page.goto(base);
  const analyze = page.locator('.column[data-stage="analyze"]');
  await analyze.getByRole("button", { name: "Pause analyze" }).click();
  await expect(analyze.getByText("Paused", { exact: true })).toBeVisible();
  await page.reload();
  await expect(analyze.getByText("Paused", { exact: true })).toBeVisible();
  await analyze.getByRole("button", { name: "Resume analyze" }).click();
  await expect(analyze.getByText("Paused", { exact: true })).toHaveCount(0);
  await expect(page.locator('.column[data-stage="done"]').getByRole("button", { name: /^Pause/ })).toHaveCount(0);
});

test("ticking a task in the brief keeps the brief scrolled", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 500 });
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Task list");
  await page.goto(`${base.split("#")[0]}#demo/T-5-tasks/brief`);
  const box = page.locator(".brief input[type=checkbox]").last();
  await box.scrollIntoViewIfNeeded();
  const brief = page.locator(".detail .brief");
  const top = await brief.evaluate((e) => e.scrollTop);
  expect(top).toBeGreaterThan(0);
  await box.click();
  await expect(box).toBeChecked();
  expect(await brief.evaluate((e) => e.scrollTop)).toBe(top);
});

test("open follow-ups show on the card and the detail chip jumps to them", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 500 });
  await page.goto(base);
  const card = page.locator(".card", { hasText: "Merged with follow-ups" });
  const chip = card.locator(".chip.follow-ups");
  await expect(chip).toHaveText("2 follow-ups");
  await expect(chip).toHaveAttribute("title", "• Deploy to prod\n• Remove the flag");

  await page.goto(`${base.split("#")[0]}#demo/T-6-follow-ups/info`);
  await page.locator(".detail header .chip.follow-ups").click();
  await expect(page.locator("#follow-ups")).toBeInViewport();
});

test("planning sessions show above the board and open in the side panel", async ({ page }) => {
  await page.goto(base);
  const bar = page.locator(".plans");
  await expect(bar.getByRole("button", { name: /Plans · 2 · 1 needs input/ })).toBeVisible();
  const live = bar.locator(".plan-card", { hasText: "Split the auth module" });
  await expect(live).toContainText("Comparing a facade with a full split");
  await expect(bar.locator(".plan-card.ended")).toContainText("Old idea");

  await live.click();
  await expect(page.locator(".detail h2")).toHaveText("Split the auth module");
  await expect(page.locator(".detail .xterm-rows")).toContainText("planning-agent-ready");
  await page.getByRole("button", { name: "Plan", exact: true }).click();
  await expect(page.locator(".detail .brief")).toContainText("Extract the token store");
  expect(page.url()).toContain("plan=demo%2Fabc123");

  await bar.locator(".plan-card", { hasText: "Old idea" }).click();
  await expect(page.getByRole("button", { name: "Resume" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Plan", exact: true })).toHaveCount(0);

  await bar.getByRole("button", { name: /Plans ·/ }).click();
  await expect(bar.locator(".plan-card")).toHaveCount(0);
  await bar.getByRole("button", { name: /Plans ·/ }).click();
  await expect(bar.locator(".plan-card")).toHaveCount(2);
});

test("moving an item into a final stage shows a toast that opens it", async ({ page }) => {
  await page.goto(base);
  await page.getByRole("tablist").getByRole("tab", { name: "research" }).click();
  await expect(page.locator(".column", { hasText: "idea" })).toContainText("Research item");
  execFileSync("python3", [resolve("../../skills/workitem-pipeline/bin/workitem-move"), join(repo, ".claude/workitems/T-4-research/workitem.md"), "findings"]);

  const toast = page.locator(".toast", { hasText: "Research item" });
  await expect(toast).toContainText("Done");
  if (process.env.TOAST_SHOTS) await page.screenshot({ path: join(process.env.TOAST_SHOTS, "done.png") });
  await toast.getByRole("button", { name: /Research item/ }).click();
  await expect(page).toHaveURL(/#demo\/T-4-research/);
  await expect(toast).toHaveCount(0);
});

test("a newly blocked item gets a toast that stays until it is dismissed", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Approve me");
  const file = join(repo, ".claude/workitems/T-3-approve/workitem.md");
  writeFileSync(file, readFileSync(file, "utf8").replace(/^agent_status: .*$/m, "agent_status: blocked: which flag?"));

  const toast = page.locator(".toast", { hasText: "Approve me" });
  await expect(toast).toContainText("Blocked: which flag?");
  await expect(toast.locator(".toast-countdown")).toHaveCount(0);
  if (process.env.TOAST_SHOTS) await page.screenshot({ path: join(process.env.TOAST_SHOTS, "blocked.png") });
  await page.waitForTimeout(7000);
  await expect(toast).toBeVisible();
  await toast.getByRole("button", { name: "Dismiss" }).click();
  await expect(toast).toHaveCount(0);
});
