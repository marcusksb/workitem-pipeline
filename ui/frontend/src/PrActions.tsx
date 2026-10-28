import { api } from "./api";
import { Item } from "./types";
import { useRun } from "./useRun";
import { IconExternal, IconGitBranch, IconGitMerge } from "./icons";

/** PR controls shown at the top of the PR tab; the backend enforces the same rules. */
export function PrActions({ item, onDone }: { item: Item; onDone: () => void }) {
  const { busy, message, error, run } = useRun(onDone);
  const inReview = item.pr_watch;

  return (
    <div className="action-bar">
      <a className="pr-link" href={item.pr} target="_blank" rel="noreferrer">
        <IconExternal />
        View PR
      </a>
      {inReview && (
        <button
          disabled={busy}
          onClick={() =>
            run("Rebase", () => api.rebase(item), `Rebase the remote branch of "${item.title}" onto its base branch? GitHub force-pushes the branch.`)
          }
        >
          <IconGitBranch />
          Rebase onto base
        </button>
      )}
      {inReview && item.pr.startsWith("http") && (
        <button
          className="good"
          disabled={busy}
          onClick={() => run("Merge", () => api.merge(item), `Squash-merge the PR for "${item.title}"? The workitem moves to done once GitHub reports it merged.`)}
        >
          <IconGitMerge />
          Merge PR
        </button>
      )}
      {message && <span>{message}</span>}
      {error && <span className="error">{error}</span>}
    </div>
  );
}
