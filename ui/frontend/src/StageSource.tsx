import { useEffect, useState } from "react";

import { api } from "./api";
import { Dialog } from "./Dialog";
import { ErrorText, Muted } from "./parts";
import { Source, StageSource as Sources } from "./types";
import { Badge } from "@/components/ui/badge";
import { NativeSelect } from "@/components/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

type Props = { label: string; stage: string; board: string; workspaces: string[]; onClose: () => void };

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

/** What a stage runs, read-only: its watch scripts and its handler skill (or inline prompt). */
export function StageSource({ label, stage, board, workspaces, onClose }: Props) {
  const [workspace, setWorkspace] = useState(workspaces[0]);
  const [sources, setSources] = useState<Sources>();
  const [error, setError] = useState<string>();
  const [tab, setTab] = useState<"script" | "prose">();

  useEffect(() => {
    setSources(undefined);
    setError(undefined);
    api.stageSource(workspace, board, stage).then(setSources, (e: unknown) => setError(e instanceof Error ? e.message : String(e)));
  }, [workspace, board, stage]);

  const hasScript = !!sources?.scripts.length;
  const hasProse = !!sources?.prose;
  const shown = tab === "script" && hasScript ? "script" : tab === "prose" && hasProse ? "prose" : hasProse ? "prose" : "script";
  return (
    <Dialog title={`${label}: what runs`} onClose={onClose} className="max-w-[760px]">
      {workspaces.length > 1 && (
        <NativeSelect className="w-auto" value={workspace} onChange={(e) => setWorkspace(e.target.value)} aria-label="Workspace">
          {workspaces.map((name) => <option key={name}>{name}</option>)}
        </NativeSelect>
      )}
      {sources && (
        <p className="text-[15px]">
          Model: {sources.model ? <code className="font-mono text-[14px]">{sources.model}</code> : <span className="text-muted-foreground">agent default</span>}
        </p>
      )}
      <Tabs value={shown} onValueChange={(value) => setTab(value as "script" | "prose")}>
        <TabsList>
          <TabsTrigger value="prose" disabled={!hasProse}>
            Skill / prompt
          </TabsTrigger>
          <TabsTrigger value="script" disabled={!hasScript}>
            Script
          </TabsTrigger>
        </TabsList>
      </Tabs>
      {error && <ErrorText>{error}</ErrorText>}
      {!sources && !error && <Muted>Loading…</Muted>}
      {sources && shown === "prose" && sources.prose && <File source={sources.prose} />}
      {sources && shown === "script" && sources.scripts.map((script, i) => <File key={i} source={script} />)}
      {sources && !hasProse && !hasScript && <Muted>This stage runs nothing.</Muted>}
    </Dialog>
  );
}
