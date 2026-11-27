import { FormEvent, useEffect, useState } from "react";

import { api } from "./api";
import { Choice, Dialog, DialogForm, Field } from "./Dialog";
import { ErrorText, Muted } from "./parts";
import { ActionSource, Source, StageSource as Sources } from "./types";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input, NativeSelect } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

type Props = { label: string; stage: string; board: string; workspaces: string[]; onClose: () => void };
type Tab = "prose" | "script" | "actions";

/** The "What runs" tab to show: the chosen one while the stage has it, else the first it has; undefined when it has none. */
export function shownTab(chosen: Tab | undefined, has: Record<Tab, boolean>): Tab | undefined {
  if (chosen && has[chosen]) return chosen;
  return (["prose", "script", "actions"] as const).find((tab) => has[tab]);
}

function File({ source }: { source: Source }) {
  const where = [source.origin, source.note].filter(Boolean).join(", ");
  return (
    <div className="grid gap-1.5">
      <p className="flex flex-wrap items-center gap-2">
        <strong className="text-[15px]">{source.name}</strong>
        {source.path && (
          <code className="font-mono text-[14px] [overflow-wrap:anywhere] text-muted-foreground" title={source.path}>
            {source.path}
          </code>
        )}
        {where && <Badge>{where}</Badge>}
      </p>
      {source.error ? (
        <ErrorText>{source.error}</ErrorText>
      ) : (
        <pre className="max-h-[55vh] overflow-auto rounded-md border border-border bg-muted p-2.5 font-mono text-[14px] whitespace-pre">{source.content}</pre>
      )}
    </div>
  );
}

/** A configured stage action (decision 89): when it shows, what it asks and passes, and what it runs. */
function Action({ action }: { action: ActionSource }) {
  const when = Object.entries(action.when);
  return (
    <section className="grid gap-1.5" aria-label={action.label}>
      <p className="flex flex-wrap items-center gap-2">
        <strong className="text-[15px]">{action.label}</strong>
        {when.length ? when.map(([key, value]) => <Badge key={key} className="font-mono">{`${key}: ${value}`}</Badge>) : <span className="text-muted-foreground">always shown</span>}
      </p>
      {action.confirm && <Muted>Asks first: {action.confirm}</Muted>}
      {action.args.length > 0 && (
        <Muted>
          Arguments: <code className="font-mono">{action.args.join(" ")}</code>
        </Muted>
      )}
      <File source={action.source} />
    </section>
  );
}

/** A stage's model and issue requirement (decision 76); the backend checks the board before it writes. */
function Settings({ workspace, board, stage, sources, onSaved }: { workspace: string; board: string; stage: string; sources: Sources; onSaved: () => void }) {
  const [model, setModel] = useState(sources.model ?? "");
  const [requireIssue, setRequireIssue] = useState(sources.require_issue);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const save = async (event: FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(undefined);
    try {
      await api.updateStage(workspace, board, stage, { model, require_issue: requireIssue });
      onSaved();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
    setBusy(false);
  };
  return (
    <DialogForm onSubmit={save}>
      <Field label="Model">
        <Input value={model} onChange={(e) => setModel(e.target.value)} placeholder="agent default" spellCheck={false} />
      </Field>
      <Choice>
        <input type="checkbox" checked={requireIssue} onChange={(e) => setRequireIssue(e.target.checked)} />
        Require a linked issue to enter this stage
      </Choice>
      <Muted>Other entry rules, the order of the stages, handlers and watchers are edited in boards/{board}.json in the workspace folder.</Muted>
      <Button type="submit" variant="primary" disabled={busy}>
        Save
      </Button>
      {error && <ErrorText>{error}</ErrorText>}
    </DialogForm>
  );
}

/** A stage's settings, and what it runs, read-only: its watch scripts, its handler skill (or inline prompt), on the PR stage the fixer skills, and its actions. */
export function StageSource({ label, stage, board, workspaces, onClose }: Props) {
  const [workspace, setWorkspace] = useState(workspaces[0]);
  const [sources, setSources] = useState<Sources>();
  const [error, setError] = useState<string>();
  const [view, setView] = useState<"settings" | "runs">("settings");
  const [tab, setTab] = useState<Tab>();
  const [loads, setLoads] = useState(0);

  useEffect(() => {
    setSources(undefined);
    setError(undefined);
    api.stageSource(workspace, board, stage).then(setSources, (e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [workspace, board, stage, loads]);

  const hasScript = !!sources?.scripts.length;
  const hasProse = !!sources?.prose || !!sources?.fixers.length;
  const hasActions = !!sources?.actions.length;
  const shown = shownTab(tab, { prose: hasProse, script: hasScript, actions: hasActions });
  return (
    <Dialog title={`${label}: stage`} onClose={onClose} className="max-w-[760px]">
      {workspaces.length > 1 && (
        <NativeSelect className="w-auto" value={workspace} onChange={(e) => setWorkspace(e.target.value)} aria-label="Workspace">
          {workspaces.map((name) => <option key={name}>{name}</option>)}
        </NativeSelect>
      )}
      <Tabs value={view} onValueChange={(value) => setView(value as "settings" | "runs")}>
        <TabsList>
          <TabsTrigger value="settings">Settings</TabsTrigger>
          <TabsTrigger value="runs">What runs</TabsTrigger>
        </TabsList>
      </Tabs>
      {error && <ErrorText>{error}</ErrorText>}
      {!sources && !error && <Muted>Loading…</Muted>}
      {sources && view === "settings" && (
        <Settings key={`${workspace}/${loads}`} workspace={workspace} board={board} stage={stage} sources={sources} onSaved={() => setLoads((n) => n + 1)} />
      )}
      {sources && view === "runs" && (
        <>
          <Tabs value={shown ?? ""} onValueChange={(value) => setTab(value as Tab)}>
            <TabsList>
              <TabsTrigger value="prose" disabled={!hasProse}>
                Skill / prompt
              </TabsTrigger>
              <TabsTrigger value="script" disabled={!hasScript}>
                Script
              </TabsTrigger>
              <TabsTrigger value="actions" disabled={!hasActions}>
                Actions
              </TabsTrigger>
            </TabsList>
          </Tabs>
          {shown === "prose" && sources.prose && <File source={sources.prose} />}
          {shown === "prose" && sources.fixers.map((fixer) => <File key={fixer.name} source={fixer} />)}
          {shown === "script" && sources.scripts.map((script, i) => <File key={i} source={script} />)}
          {shown === "actions" && (
            <>
              {sources.actions.map((action) => <Action key={action.label} action={action} />)}
              <Muted>Actions are edited in boards/{board}.json in the workspace folder.</Muted>
            </>
          )}
          {!shown && <Muted>This stage runs nothing.</Muted>}
        </>
      )}
    </Dialog>
  );
}
