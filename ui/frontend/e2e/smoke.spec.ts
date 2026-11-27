// Runs the built backend against a real tmux session; needs `cargo build` and `npm run build` first.
import { expect, test } from "@playwright/test";
import { ChildProcess, execFileSync, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const port = 7392;
const socket = `wi-e2e-${process.pid}`;
let server: ChildProcess;
let base = "";
let repo = "";
/** The workspace folder; the first test creates it in the UI, then `seed` fills it. */
let ws = "";
let home = "";

/** Items, a second board, learnings and plans, written into the workspace once the UI created it. */
function seed() {
  const write = (path: string, text: string) => {
    mkdirSync(join(path, ".."), { recursive: true });
    writeFileSync(path, text);
  };
  write(
    join(ws, "T-1-demo/workitem.md"),
    "---\ntitle: Demo blocked item\ntype: bug\nstage: analyze\nboard: default\napproved: false\nagent_status: blocked: need your answer\nissue: T-1\npr: https://github.com/demo/repo/pull/42\npr_state: open\npr_ci: failing\n---\n\n# Demo\n\nHello brief text.\n",
  );
  write(
    join(ws, "T-2-gate/workitem.md"),
    "---\ntitle: Brief waiting\ntype: feature\nstage: ready_for_development\nboard: default\napproved: false\nagent_status: ok\nissue: T-2\n---\n\n# Gate\n",
  );
  write(join(ws, "T-2-gate/notes/findings.md"), "# Findings\n\n- the cache is stale\n");
  write(join(ws, "T-2-gate/log.txt"), "line one\nline two\n");
  write(join(ws, "T-2-gate/drafts.html"), "<p>Draft → ä</p><script>document.body.append('scripted')</script>");
  write(
    join(ws, "T-3-approve/workitem.md"),
    "---\ntitle: Approve me\ntype: feature\nstage: ready_for_development\nboard: default\napproved: false\nagent_status: ok\nissue: T-3\n---\n\n# Approve\n",
  );
  write(
    join(ws, "boards/research.json"),
    JSON.stringify({ repo, brief_sections: [], entry: "idea", stages: [{ name: "idea", next: ["findings"] }, { name: "findings", next: [] }] }),
  );
  write(
    join(ws, "T-4-research/workitem.md"),
    "---\ntitle: Research item\ntype: feature\nstage: idea\nboard: research\nagent_status: ok\nissue: T-4\npr: https://github.com/demo/repo/pull/43\npr_state: open\npr_ci: pending\n---\n\n# Research\n",
  );
  const findings = Array.from({ length: 6 }, (_, i) => `- [ ] **Finding ${i}**\\\n  What is wrong and how to fix it.\n`).join("\n");
  write(
    join(ws, "T-5-tasks/workitem.md"),
    `---\ntitle: Task list\ntype: review\nstage: done\nboard: default\nagent_status: ok\nissue: T-5\n---\n\n# Review\n\n${"A paragraph.\n\n".repeat(30)}${findings}`,
  );
  write(
    join(ws, "T-6-follow-ups/workitem.md"),
    `---\ntitle: Merged with follow-ups\ntype: feature\nstage: done\nboard: default\nagent_status: ok\nissue: T-6\n---\n\n# Done\n\n${"A paragraph.\n\n".repeat(40)}## Follow-ups\n\n- [ ] Deploy to prod\n- [x] Tell support\n- [ ] Remove the flag\n`,
  );
  write(
    join(ws, "T-7-ui/workitem.md"),
    "---\ntitle: Show the new field\ntype: feature\nstage: backlog\nboard: research\nagent_status: ok\nissue: T-7\ndepends_on: T-1-demo\n---\n\n# UI\n",
  );
  write(
    join(ws, "T-8-child/workitem.md"),
    "---\ntitle: Split off part\ntype: feature\nstage: idea\nboard: research\nagent_status: ok\nparent: T-2-gate\n---\n\n# Child\n",
  );
  write(
    join(ws, "boards/default/learnings.md"),
    "# Learnings\n\n- Never mock tmux in tests. (source: https://github.com/demo/repo/pull/42#discussion_r1)\n",
  );
  write(
    join(ws, "plans/abc123.md"),
    "---\ntopic: Split the auth module\nsummary: Comparing a facade with a full split\nstatus: waiting\nworkitems: T-2-gate\nclaude_session: 1\ncreated: 2\n---\n\n## Prompt\n\nSplit the auth module\n\n## Plan\n\n1. Extract the token store\n",
  );
  write(
    join(ws, "plans/def456.md"),
    "---\ntopic: Old idea\nsummary: Parked\nstatus: ended\nclaude_session: 2\ncreated: 1\n---\n\n## Prompt\n\nOld idea\n",
  );
}

test.beforeAll(async () => {
  const dir = mkdtempSync(join(tmpdir(), "wi-e2e-"));
  repo = join(dir, "repo");
  mkdirSync(repo, { recursive: true });
  home = join(dir, "home");
  ws = join(home, "workspaces/demo");
  // Sessions the UI started carry their workspace folder; the folder does not have to exist yet.
  const tag = [";", "set-option", "@workitem_workspace", ws];
  execFileSync("tmux", ["-L", socket, "new-session", "-d", "-s", "wi-plan-abc123", "-c", repo, "echo planning-agent-ready; sleep 600", ...tag]);
  execFileSync("tmux", [
    "-L", socket, "new-session", "-d", "-s", "wi-T-1-demo", "-c", repo,
    "echo 'agent: which db do you want?'; read a; echo got:$a; sleep 600", ...tag,
  ]);

  server = spawn(resolve("../backend/target/debug/workload"), [], {
    env: {
      ...process.env,
      WORKITEM_UI_HOME: home,
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

test("add a workspace, find the blocked item, answer the agent in the browser terminal", async ({ page }) => {
  await page.goto(base);
  await expect(page.getByText("No workspaces yet")).toBeVisible();
  expect(page.url()).not.toContain("token");

  await page.getByRole("button", { name: /^Workspace/ }).click();
  await page.getByRole("menuitem", { name: "Add workspace" }).click();
  await page.getByPlaceholder("content-delivery").fill("demo");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByText("No boards in demo yet")).toBeVisible();
  seed();

  // The default board is the bundled one for the repo.
  await page.getByRole("button", { name: "New board" }).click();
  await page.getByPlaceholder("backend").fill("default");
  await page.getByPlaceholder("/absolute/path/to/repo").fill(repo);
  await page.getByRole("button", { name: "Add", exact: true }).click();

  const board = page.locator(".board");
  await expect(board.getByText("Demo blocked item")).toBeVisible();
  expect(JSON.parse(readFileSync(join(ws, "boards/default.json"), "utf8")).repo).toBe(realpathSync(repo));
  const blocked = board.locator(".card.blocked", { hasText: "Demo blocked item" });
  await expect(blocked).toBeVisible();
  // The card's PR pill is coloured by its merge state, then approval and CI icons; its status line says why the agent stopped.
  await expect(blocked.getByRole("link", { name: "#42 merge unknown" })).toHaveAttribute("href", "https://github.com/demo/repo/pull/42");
  await expect(blocked.getByRole("img", { name: /^CI failing/ })).toBeVisible();
  await expect(blocked.getByRole("img", { name: /^CI failing/ })).not.toHaveClass(/animate-spin/);
  await expect(blocked.locator(".agent-status")).toHaveText("Blocked");
  await board.getByText("Demo blocked item").click();

  const terminal = page.locator(".detail .xterm-rows");
  await expect(terminal).toContainText("which db do you want?");
  await page.locator(".detail .xterm").click();
  await page.keyboard.type("postgres\n");
  await expect(terminal).toContainText("got:postgres");

  await expect(page.locator(".detail").getByRole("link", { name: "#42 merge unknown" })).toHaveAttribute("href", "https://github.com/demo/repo/pull/42");
  await page.getByRole("tab", { name: "PR", exact: true }).click();
  await expect(page.locator(".brief .pr-link")).toHaveAttribute("href", "https://github.com/demo/repo/pull/42");
  await page.getByRole("tab", { name: "Brief", exact: true }).click();
  await expect(page.getByText("Hello brief text.")).toBeVisible();

  // Closing the panel detaches; the agent session survives.
  await page.getByRole("button", { name: "Close", exact: true }).click();
  execFileSync("tmux", ["-L", socket, "has-session", "-t", "wi-T-1-demo"]);
});

test("approve at the gate from the gate bar", async ({ page }) => {
  await page.goto(base);
  const gateColumn = page.locator(".column", { hasText: "ready for development" });

  // Approving moves the item on, to the stage the gate names.
  await gateColumn.getByText("Approve me").click();
  await expect(page.locator(".detail").getByRole("button", { name: "Discuss brief" })).toHaveCount(2);
  // Without a session, the tab row offers to start one.
  await expect(page.locator(".detail").getByRole("button", { name: "Start agent session", exact: true })).toBeVisible();
  await page.locator(".detail").getByRole("button", { name: "Approve", exact: true }).click();
  await expect(page.locator(".column", { hasText: "in development" })).toContainText("Approve me");
  await expect(gateColumn).not.toContainText("Approve me");
});

test("deep links and history navigate items and tabs", async ({ page }) => {
  await page.goto(base);
  await page.locator(".board").getByText("Demo blocked item").click();
  await expect(page).toHaveURL(/#demo\/T-1-demo$/);
  await expect(page.getByRole("tab", { name: "Terminal" })).toHaveAttribute("aria-selected", "true");
  await page.getByRole("tab", { name: "Info" }).click();
  await expect(page).toHaveURL(/#demo\/T-1-demo\/info$/);
  await expect(page.getByRole("button", { name: "Remove", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Start agent session", exact: true })).toHaveCount(0);

  await page.goBack();
  await expect(page).toHaveURL(/#demo\/T-1-demo$/);
  // T-1 is blocked with a live session, so the default tab is the terminal.
  await expect(page.locator(".detail .xterm-rows")).toBeVisible();
  await page.goForward();
  await expect(page).toHaveURL(/#demo\/T-1-demo\/info$/);

  await page.getByRole("button", { name: "Close", exact: true }).click();
  await expect(page.locator(".detail")).toHaveCount(0);

  await page.goto(`${base.split("#")[0]}#demo/T-1-demo/terminal`);
  await expect(page.locator(".detail .xterm-rows")).toBeVisible();

  // Escape belongs to the agent while the terminal has focus, so the item stays open.
  await page.locator(".detail .xterm").click();
  await page.keyboard.press("Escape");
  await expect(page.locator(".detail")).toHaveCount(1);

  // A dialog opened from the board renders outside it, but a click in it does not close the item.
  await page.getByRole("button", { name: "analyze settings" }).click();
  const source = page.getByRole("dialog", { name: "analyze: stage" });
  await source.getByRole("heading", { name: "analyze: stage" }).click();
  await page.keyboard.press("Escape");
  await expect(source).toHaveCount(0);
  await expect(page.locator(".detail")).toHaveCount(1);

  // A click on the board outside any card closes the item like the Close button.
  await page.locator(".column h2").first().click({ position: { x: 2, y: 2 } });
  await expect(page.locator(".detail")).toHaveCount(0);

  // So does a click on the sidebar outside its controls.
  await page.locator(".card").first().click();
  await expect(page.locator(".detail")).toHaveCount(1);
  await page.locator("nav").first().click({ position: { x: 2, y: 2 } });
  await expect(page.locator(".detail")).toHaveCount(0);

  // Escape closes the item like a background click, once the terminal no longer has focus.
  await page.locator(".card").first().click();
  await expect(page.locator(".detail")).toHaveCount(1);
  await page.locator(".detail h2").click();
  await page.keyboard.press("Escape");
  await expect(page.locator(".detail")).toHaveCount(0);

  // In the brief editor it cancels the edit first: with no changes it just leaves the editor.
  await page.locator(".card").first().click();
  await page.getByRole("tab", { name: "Brief", exact: true }).click();
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await expect(page.locator(".detail textarea")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.locator(".detail textarea")).toHaveCount(0);
  await expect(page.locator(".detail")).toHaveCount(1);

  // A changed brief asks before Escape from outside the editor closes the panel and drops it.
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.locator(".detail textarea").fill("A change");
  await page.locator(".detail h2").click();
  let asked = "";
  page.once("dialog", (dialog) => {
    asked = dialog.message();
    void dialog.dismiss();
  });
  await page.keyboard.press("Escape");
  expect(asked).toContain("Discard");
  await expect(page.locator(".detail")).toHaveCount(1);

  // The Info draft asks the same way once a field changed.
  await page.getByRole("tab", { name: "Info", exact: true }).click();
  await page.getByRole("button", { name: "Edit", exact: true }).click();
  await page.locator(".detail input").first().fill("changed");
  page.once("dialog", (dialog) => void dialog.dismiss());
  await page.keyboard.press("Escape");
  await expect(page.locator(".detail input").first()).toBeVisible();
  await expect(page.locator(".detail")).toHaveCount(1);
});

test("a workspace's boards are tabs in the sidebar and the chosen board stays in the URL", async ({ page }) => {
  await page.goto(base);
  const tabs = page.getByRole("tablist", { name: "Boards" });
  await expect(tabs.getByRole("tab", { name: /default/ })).toHaveAttribute("aria-selected", "true");

  // Each board shows how many items it has, and how many of them are blocked.
  await expect(tabs.getByRole("tab", { name: /default/ })).toHaveText("default15");
  await expect(tabs.getByRole("tab", { name: /research/ })).toHaveText("research3");
  // Loaded, the default board has no research column (its child of T-2-gate still shows under the parent).
  await expect(page.locator('.column[data-stage="idea"]')).toHaveCount(0);
  await expect(page.locator(".top-bar h1")).toHaveText("default");
  await expect(page.locator(".top-bar")).toContainText("repo");
  await tabs.getByRole("tab", { name: /research/ }).click();
  await expect(page).toHaveURL(/#\?workspace=demo&board=research$/);
  await expect(page.locator(".column", { hasText: "idea" })).toContainText("Research item");
  // A running CI spins its icon (unless the user prefers reduced motion); a failing one stays still (see the first test).
  await expect(page.locator(".card", { hasText: "Research item" }).getByRole("img", { name: "CI running" })).toHaveClass(/motion-safe:animate-spin/);
  await expect(page.locator(".column", { hasText: "backlog" })).toHaveCount(0);

  // The choice survives a reload, and an item's deep link keeps it.
  await page.reload();
  await expect(tabs.getByRole("tab", { name: /research/ })).toHaveAttribute("aria-selected", "true");
  await page.locator(".column", { hasText: "idea" }).getByText("Research item").click();
  await expect(page).toHaveURL(/#demo\/T-4-research\?board=research$/);
});

test("the top bar switches Board and List and opens New workitem on the board on screen", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Demo blocked item");
  await page.goto(`${base.split("#")[0]}#?workspace=demo&board=research`);
  const view = page.getByRole("group", { name: "View" });
  await expect(view.getByRole("button", { name: "board" })).toHaveAttribute("aria-pressed", "true");
  await view.getByRole("button", { name: "list" }).click();
  await expect(page.locator(".board.list")).toContainText("Research item");
  await page.reload();
  await expect(view.getByRole("button", { name: "list" })).toHaveAttribute("aria-pressed", "true");
  await view.getByRole("button", { name: "board" }).click();
  await expect(page.locator(".board.list")).toHaveCount(0);

  await page.getByRole("button", { name: "New workitem" }).click();
  const dialog = page.getByRole("dialog", { name: "New workitem in demo" });
  await expect(dialog.getByRole("combobox")).toHaveValue("research");
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("the sidebar opens the workspace's settings, watchers and a new plan", async ({ page }) => {
  await page.goto(base);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.getByRole("dialog", { name: "Settings of demo" }).getByLabel("Agent")).toBeFocused();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Watchers" }).click();
  await expect(page.getByRole("dialog", { name: "Watchers of demo" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "New plan" }).click();
  await expect(page.getByRole("dialog", { name: "New planning session" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
});

test("at phone width every shell action stays reachable without horizontal scrolling", async ({ page }) => {
  await page.setViewportSize({ width: 375, height: 740 });
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Demo blocked item");
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(375);

  const opens = async (button: string, dialog: string) => {
    await page.getByRole("button", { name: button, exact: true }).click();
    await expect(page.getByRole("dialog", { name: dialog })).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(page.getByRole("dialog")).toHaveCount(0);
  };
  await opens("New board", "Add a board to demo");
  await opens("New plan", "New planning session");
  await opens("Learnings", "Learnings in demo · default");
  await opens("Watchers", "Watchers of demo");
  await opens("Settings", "Settings of demo");
  await opens("New workitem", "New workitem in demo");
  await page.getByRole("button", { name: /^Workspace/ }).click();
  await page.getByRole("menuitem", { name: "Add workspace" }).click();
  await expect(page.getByRole("dialog", { name: "Add workspace" })).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: /Search/ }).click();
  await expect(page.getByRole("combobox", { name: "Search workitems" })).toBeVisible();
  await page.keyboard.press("Escape");
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(375);
});

test("the sidebar collapses to a rail on narrow windows and the toggle overrides it", async ({ page }) => {
  await page.goto(base);
  const sidebar = page.locator(".sidebar");
  await expect(sidebar).toHaveAttribute("data-collapsed", "false");
  await page.setViewportSize({ width: 1000, height: 740 });
  await expect(sidebar).toHaveAttribute("data-collapsed", "true");
  // The rail keeps every entry, by name, with the board counts left out.
  await expect(page.getByRole("tab", { name: /research/ })).toBeVisible();
  await expect(page.getByText("15", { exact: true })).toBeHidden();

  await page.getByRole("button", { name: "Expand sidebar" }).click();
  await expect(sidebar).toHaveAttribute("data-collapsed", "false");
  await page.getByRole("button", { name: "Collapse sidebar" }).click();
  await expect(sidebar).toHaveAttribute("data-collapsed", "true");

  // At phone width the expanded sidebar floats; picking a board closes it.
  await page.setViewportSize({ width: 375, height: 740 });
  await page.getByRole("button", { name: "Expand sidebar" }).click();
  await page.getByRole("tab", { name: /research/ }).click();
  await expect(page).toHaveURL(/board=research/);
  await expect(sidebar).toHaveAttribute("data-collapsed", "true");
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(375);
});

test("expand on hover keeps the sidebar a rail that floats open under the pointer", async ({ page }) => {
  await page.goto(base);
  const sidebar = page.locator(".sidebar");
  const hover = page.getByRole("button", { name: "Expand on hover" });
  await hover.click();
  await expect(hover).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: /^(Collapse|Expand) sidebar$/ })).toBeDisabled();
  // Still open while the pointer is on it; a rail once it leaves, and the board does not move.
  await expect(sidebar).toHaveAttribute("data-collapsed", "false");
  const left = await page.locator(".content").boundingBox();
  await page.mouse.move(800, 400);
  await expect(sidebar).toHaveAttribute("data-collapsed", "true");
  expect((await page.locator(".content").boundingBox())?.x).toBe(left?.x);
  await sidebar.hover();
  await expect(sidebar).toHaveAttribute("data-collapsed", "false");

  // The choice survives a reload; turning it off keeps the sidebar open.
  await page.reload();
  await page.mouse.move(800, 400);
  await expect(sidebar).toHaveAttribute("data-collapsed", "true");
  await hover.click();
  await expect(hover).toHaveAttribute("aria-pressed", "false");
  await page.mouse.move(800, 400);
  await expect(sidebar).toHaveAttribute("data-collapsed", "false");
});

test("the selected workspace stays in the URL and survives a reload", async ({ page }) => {
  await page.goto(base);
  const switcher = page.getByRole("button", { name: /^Workspace/ });
  await switcher.click();
  await page.getByRole("menuitem", { name: "Add workspace" }).click();
  await page.getByPlaceholder("content-delivery").fill("other");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(switcher).toHaveAccessibleName("Workspace: other");
  await expect(page).toHaveURL(/#\?workspace=other$/);
  await expect(page.locator(".board")).not.toContainText("Demo blocked item");
  await page.reload();
  await expect(switcher).toHaveAccessibleName("Workspace: other");
  await switcher.click();
  await expect(page.getByRole("menuitemradio", { name: "other" })).toHaveAttribute("aria-checked", "true");
  await page.getByRole("menuitemradio", { name: "demo" }).click();
  await expect(page).toHaveURL(/#\?workspace=demo$/);
  await expect(page.locator(".board")).toContainText("Demo blocked item");
});

test("a card that waits on an unmerged item opens its Depends on tab, which opens the dependency on its own board", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Demo blocked item");
  await page.goto(`${base.split("#")[0]}#?workspace=demo&board=research`);
  const chip = page.locator(".card", { hasText: "Show the new field" }).locator(".chip.waits-on");
  await expect(chip).toHaveText("depends on 1");
  await chip.click();
  await expect(page).toHaveURL(/#demo\/T-7-ui\/dependencies/);
  const deps = page.locator(".dependencies");
  await expect(deps).toContainText("default");
  await expect(deps).toContainText("not merged");
  await deps.locator(".child-head .link").first().click();
  await expect(page).toHaveURL(/#demo\/T-1-demo\?board=default$/);
  await expect(page.getByRole("tablist", { name: "Boards" }).getByRole("tab", { name: /default/ })).toHaveAttribute("aria-selected", "true");

  // The tab edits the list; Info no longer shows it.
  await page.goto(`${base.split("#")[0]}#demo/T-7-ui/info`);
  await expect(page.locator(".dependencies")).toHaveCount(0);
  await page.goto(`${base.split("#")[0]}#demo/T-7-ui/dependencies`);
  await expect(deps.locator("li")).toContainText("Demo blocked item");
  await deps.getByRole("button", { name: "Remove T-1-demo" }).click();
  await expect(deps).toContainText("nothing");
  await deps.getByRole("combobox", { name: "Add a dependency" }).selectOption("T-2-gate");
  await expect(deps.locator("li")).toContainText("Brief waiting");
  expect(readFileSync(join(ws, "T-7-ui/workitem.md"), "utf8")).toContain("depends_on: T-2-gate\n");
});

test("a card's parent badge opens the parent on its own board", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Demo blocked item");
  await page.goto(`${base.split("#")[0]}#?workspace=demo&board=research`);
  const badge = page.locator(".card", { hasText: "Split off part" }).getByRole("button", { name: "Open parent T-2-gate" });
  await expect(badge).toHaveText("parent");
  await expect(badge).toHaveAttribute("title", "split from T-2-gate");
  await badge.click();
  await expect(page).toHaveURL(/#demo\/T-2-gate\?board=default$/);
  await expect(page.locator(".detail")).toContainText("Brief waiting");
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
  await expect(page).toHaveURL(/#demo\/T-3-approve$/);
});

test("the palette runs New plan and New workitem", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board").getByText("Demo blocked item")).toBeVisible();
  const search = page.getByRole("combobox", { name: "Search workitems" });

  await page.keyboard.press("ControlOrMeta+k");
  await expect(page.getByRole("group", { name: "Actions" }).getByRole("option")).toContainText(["New workitem", "New plan"]);
  await search.fill("new plan");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog", { name: "New planning session" }).getByRole("textbox")).toBeFocused();
  await expect(search).toHaveCount(0);
  await page.keyboard.press("Escape");

  await page.keyboard.press("ControlOrMeta+k");
  await search.fill("new workitem");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("dialog", { name: "New workitem in demo" })).toBeVisible();
});

test("the Files tab lists the workitem folder and previews a file", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Brief waiting");
  await page.goto(`${base.split("#")[0]}#demo/T-2-gate/files`);
  const list = page.locator(".file-list");
  await expect(list.getByRole("button")).toHaveText([/drafts\.html/, /log\.txt/, /notes\/findings\.md/, /workitem\.md/]);
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
  // Last: the tall frame scrolls the list under the tabs.
  await list.getByRole("button", { name: /drafts\.html/ }).click();
  await expect(page.locator(".file-preview iframe")).toHaveAttribute("sandbox", "allow-scripts");
  const frame = page.frameLocator(".file-preview iframe").locator("body");
  await expect(frame).toContainText("Draft → ä");
  await expect(frame).toContainText("scripted");
});

test("the Learnings button shows and edits the learnings of the board on screen", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Brief waiting");
  await page.getByRole("button", { name: "Learnings", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Learnings in demo · default" });
  await expect(dialog.getByRole("listitem")).toContainText("Never mock tmux in tests.");
  await expect(dialog.getByRole("link", { name: /pull\/42/ })).toHaveAttribute("target", "_blank");
  await expect(dialog.locator(".source-path code")).toContainText("learnings.md");
  await dialog.getByRole("button", { name: "Close" }).click();

  // Another board has its own file, empty until you add a rule.
  await page.getByRole("tablist", { name: "Boards" }).getByRole("tab", { name: /research/ }).click();
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
  expect(readFileSync(join(ws, "boards/research/learnings.md"), "utf8")).toBe("# Learnings\n\n- Flag only real bugs.\n");
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
  await page.locator(".detail .chip.follow-ups").click();
  await expect(page.locator("#follow-ups")).toBeInViewport();
});

test("planning sessions show in the sidebar and open in the side panel", async ({ page }) => {
  await page.goto(base);
  const bar = page.locator(".plans");
  await expect(bar.getByRole("button", { name: /Plans · 2 · 1 needs input/ })).toBeVisible();
  const live = bar.locator(".plan-card", { hasText: "Split the auth module" });
  await expect(live).toHaveAttribute("title", /Comparing a facade with a full split/);
  await expect(bar.locator(".plan-card.ended")).toContainText("Old idea");

  await live.click();
  await expect(page.locator(".detail").getByRole("heading", { level: 2, name: "Split the auth module", exact: true })).toBeVisible();
  await expect(page.locator(".detail .xterm-rows")).toContainText("planning-agent-ready");
  await page.getByRole("tab", { name: "Plan", exact: true }).click();
  await expect(page.locator(".detail .brief")).toContainText("Extract the token store");
  expect(page.url()).toContain("plan=demo%2Fabc123");
  await page.getByRole("tab", { name: /Workitems/ }).click();
  await page.locator(".detail").getByRole("button", { name: "Brief waiting" }).click();
  await expect(page).toHaveURL(/#demo\/T-2-gate/);
  await page.goBack();

  await bar.locator(".plan-card", { hasText: "Old idea" }).click();
  await expect(page.getByRole("button", { name: "Resume" })).toBeVisible();
  await expect(page.getByRole("tab", { name: "Plan", exact: true })).toHaveCount(0);

  await bar.getByRole("button", { name: /Plans ·/ }).click();
  await expect(bar.locator(".plan-card")).toHaveCount(0);
  await bar.getByRole("button", { name: /Plans ·/ }).click();
  await expect(bar.locator(".plan-card")).toHaveCount(2);
});

test("the item and plan panels resize with the mouse and remember it", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Brief waiting");
  await page.goto(`${base.split("#")[0]}#demo/T-2-gate`);
  const panel = page.locator(".detail");
  await expect(panel.getByRole("heading", { name: "Brief waiting" })).toBeVisible();
  // Let the slide-in settle, so the handle is where it stays.
  await panel.evaluate((el) => Promise.all(el.getAnimations().map((animation) => animation.finished)));
  const before = (await panel.boundingBox())!;
  const handle = panel.locator("[data-panel-handle]");
  const box = (await handle.boundingBox())!;

  // Drag the left edge to the left: the panel grows, and the integer width is stored.
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 - 120, box.y + box.height / 2, { steps: 5 });
  await page.mouse.up();
  const dragged = (await panel.boundingBox())!;
  expect(dragged.width).toBeGreaterThan(before.width + 100);
  const stored = await page.evaluate(() => localStorage.getItem("workload.panel.width"));
  expect(Number(stored)).toBe(Math.round(dragged.width));

  // The width survives a reload.
  await page.reload();
  await expect(page.locator(".detail").getByRole("heading", { name: "Brief waiting" })).toBeVisible();
  expect((await page.locator(".detail").boundingBox())!.width).toBeCloseTo(dragged.width, 0);

  // The plan panel reads the same width. (The item panel is still sliding out on this hash change.)
  await page.goto(`${base.split("#")[0]}#?plan=demo%2Fabc123`);
  const planPanel = page.getByRole("complementary", { name: "Plan" });
  await expect(planPanel.getByRole("heading", { level: 2, name: "Split the auth module", exact: true })).toBeVisible();
  expect((await planPanel.boundingBox())!.width).toBeCloseTo(dragged.width, 0);

  // Double-clicking the handle forgets the width and restores the responsive default.
  await planPanel.locator("[data-panel-handle]").dblclick();
  await expect.poll(async () => (await planPanel.boundingBox())!.width).toBeCloseTo(before.width, 0);
  expect(await page.evaluate(() => localStorage.getItem("workload.panel.width"))).toBeNull();
});

test("moving an item into a final stage shows a toast that opens it", async ({ page }) => {
  await page.goto(base);
  await page.getByRole("tablist", { name: "Boards" }).getByRole("tab", { name: /research/ }).click();
  await expect(page.locator(".column", { hasText: "idea" })).toContainText("Research item");
  execFileSync("python3", [resolve("../../skills/workitem-pipeline/bin/workitem-move"), join(ws, "T-4-research/workitem.md"), "findings"]);

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
  const file = join(ws, "T-3-approve/workitem.md");
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

test("toasts stay clear of the open detail panel", async ({ page }) => {
  await page.goto(base);
  await expect(page.locator(".board")).toContainText("Task list");
  await page.goto(`${base.split("#")[0]}#demo/T-5-tasks/info`);
  const panel = page.locator(".detail");
  await expect(panel).toContainText("Task list");
  const file = join(ws, "T-6-follow-ups/workitem.md");
  writeFileSync(file, readFileSync(file, "utf8").replace(/^agent_status: .*$/m, "agent_status: blocked: deploy now?"));

  const toast = page.locator(".toast", { hasText: "Merged with follow-ups" });
  await expect(toast).toContainText("Blocked: deploy now?");
  // Let the slide-in finish, so the box is where the toast rests.
  await page.waitForTimeout(700);
  const a = (await toast.boundingBox())!;
  const b = (await panel.boundingBox())!;
  const overlaps = a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
  expect(overlaps).toBe(false);
});

test("a create session is a toast: spinner, amber while waiting, then a link to what it created", async ({ page }) => {
  const tmux = (...args: string[]) => execFileSync("tmux", ["-L", socket, ...args]);
  tmux("new-session", "-d", "-s", "wi-new-e2e", "sleep 600", ";", "set-option", "@workitem_workspace", ws);
  await page.goto(base);
  const toast = page.locator(".toast", { hasText: "Creating workitem…" });
  await expect(toast.locator(".toast-spinner")).toBeVisible();
  await expect(toast.getByRole("link", { name: "Open terminal ↗" })).toHaveAttribute("href", /wi-new-e2e/);
  await expect(toast.getByRole("button", { name: "Dismiss" })).toHaveCount(0);

  tmux("set-option", "-t", "=wi-new-e2e:", "@workitem_waiting", "1");
  await expect(page.locator(".toast.tone-wait")).toContainText("Needs your answer");

  mkdirSync(join(ws, "T-9-made"), { recursive: true });
  writeFileSync(join(ws, "T-9-made/workitem.md"), "---\ntitle: Made by the toast test\ntype: feature\nstage: analyze\nboard: default\nagent_status: ok\nissue: T-9\ncreated_by: wi-new-e2e\n---\n\n# Made\n");
  tmux("kill-session", "-t", "=wi-new-e2e");
  const created = page.locator(".toast.tone-good", { hasText: "Created: Made by the toast test" });
  await created.getByRole("button", { name: "Created: Made by the toast test" }).click();
  await expect(page).toHaveURL(/#demo\/T-9-made/);
  await expect(created).toHaveCount(0);
});
