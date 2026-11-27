import { ReactElement } from "react";
import { describe, expect, it, vi } from "vitest";

import { api } from "./api";
import { StageActionBar } from "./StageActions";
import { item } from "./testFixtures";

type Props = { children?: unknown; onClick?: () => void; disabled?: boolean };

/** The buttons the bar renders, as elements: one per action, then the message and error spans. */
const buttons = (bar: ReactElement) => ((bar.props as Props).children as unknown[])[0] as ReactElement<Props>[];

describe("stage actions", () => {
  const actions = [
    { id: "Rebase onto base", label: "Rebase onto base", confirm: null, starts_session: false },
    { id: "Merge PR", label: "Merge PR", confirm: "Squash-merge the PR?", starts_session: false },
  ];
  const runner = (over = {}) => ({ busy: false, message: undefined, error: undefined, run: vi.fn(), ...over });

  const onSession = vi.fn();

  it("renders one button per action and runs it through api.runAction with its confirm text", () => {
    const run = runner();
    const shown = item({ actions });
    const list = buttons(StageActionBar({ item: shown, runner: run, onSession }));
    expect(list.map((b) => b.props.children)).toEqual(["Rebase onto base", "Merge PR"]);

    const runAction = vi.spyOn(api, "runAction").mockResolvedValue(undefined);
    list[1].props.onClick!();
    const [label, action, confirmText] = run.run.mock.calls[0];
    expect([label, confirmText]).toEqual(["Merge PR", "Squash-merge the PR?"]);
    void action();
    expect(runAction).toHaveBeenCalledWith(shown, "Merge PR");

    list[0].props.onClick!();
    expect(run.run.mock.calls[1][2]).toBeUndefined();
  });

  it("disables the buttons while one runs", () => {
    expect(buttons(StageActionBar({ item: item({ actions }), runner: runner({ busy: true }), onSession })).every((b) => b.props.disabled)).toBe(true);
  });

  const withSkill = [...actions, { id: "Check", label: "Check", confirm: null, starts_session: true }];

  it("disables only a session-starting action while the item has a session or is not ok", () => {
    const disabled = (over: object) => buttons(StageActionBar({ item: item({ actions: withSkill, ...over }), runner: runner(), onSession })).map((b) => b.props.disabled);
    expect(disabled({ session: true })).toEqual([false, false, true]);
    expect(disabled({ agent_status: "blocked: x" })).toEqual([false, false, true]);
    expect(disabled({})).toEqual([false, false, false]);
  });

  it("follows a skill action into its session, and only that", async () => {
    const run = runner();
    const list = buttons(StageActionBar({ item: item({ actions: withSkill }), runner: run, onSession }));
    const runAction = vi.spyOn(api, "runAction").mockResolvedValueOnce(undefined).mockResolvedValueOnce({ session: "wi-x" });
    list[0].props.onClick!();
    list[2].props.onClick!();
    await run.run.mock.calls[0][1]();
    expect(onSession).not.toHaveBeenCalled();
    await run.run.mock.calls[1][1]();
    expect(onSession).toHaveBeenCalledOnce();
    runAction.mockRestore();
  });
});
