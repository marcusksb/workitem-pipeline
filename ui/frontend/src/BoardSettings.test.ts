import { describe, expect, it } from "vitest";

import { formOf, settingsOf, workitemCount } from "./BoardSettings";
import { Pipeline } from "./types";

const pipeline: Pipeline = { repo: "/code/app", entry: "backlog", stages: [], order: ["issue_status desc", "title"], reviewers: "acme/web" };

describe("board settings form", () => {
  it("is prefilled from the board, with absent keys empty and order one entry per line", () => {
    expect(formOf(pipeline)).toEqual({ label: "", entry: "backlog", order: "issue_status desc\ntitle", issue_project: "", reviewers: "acme/web" });
    expect(formOf({ ...pipeline, label: "Backend", issue_project: "MNT" })).toMatchObject({ label: "Backend", issue_project: "MNT" });
  });

  it("sends order split into lines without blank ones, and never repo", () => {
    const settings = settingsOf({ ...formOf(pipeline), order: " created desc \n\n  \ntitle\n" });
    expect(settings.order).toEqual(["created desc", "title"]);
    expect(settings).not.toHaveProperty("repo");
    expect(settingsOf({ ...formOf(pipeline), order: "" }).order).toEqual([]);
  });

  it("counts the workitems the delete confirmation names", () => {
    expect([0, 1, 2].map(workitemCount)).toEqual(["0 workitems", "1 workitem", "2 workitems"]);
  });
});
