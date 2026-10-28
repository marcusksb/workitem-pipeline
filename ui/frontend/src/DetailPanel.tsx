import { useEffect, useState } from "react";
import { api, itemTerminal } from "./api";
import { isBlocked, KIND_ICONS, stageLabel, stripFrontmatter } from "./boardState";
import { Item } from "./types";
import { Actions } from "./Actions";
import { AgentActions } from "./AgentActions";
import { BriefActions } from "./BriefActions";
import { BriefEditor } from "./BriefEditor";
import { BriefView } from "./BriefView";
import { PrActions } from "./PrActions";
import { PrConversation } from "./PrConversation";
import { Files } from "./Files";
import { Info } from "./Info";
import { LinkBadges } from "./LinkBadges";
import { ReviewStrip } from "./ReviewStrip";
import { Tab } from "./url";
import { Terminal } from "./Terminal";
import { usePolling } from "./usePolling";
import { IconExternal, IconInfo, IconFileText, IconFolder, IconGitPullRequest, IconSliders, IconTerminalSquare, IconX } from "./icons";

type Props = {
  item: Item;
  /** Items of the visible projects, used to find the parent and children. */
  items: Item[];
  onSelect: (item: Item) => void;
  /** Tab from the URL hash; unset falls back to the state-dependent default. */
  tab?: Tab;
  onTab: (tab: Tab) => void;
  onClose: () => void;
  onUnauthorized: () => void;
};

export function DetailPanel({ item, items, onSelect, tab: routeTab, onTab, onClose, onUnauthorized }: Props) {
  const key = `${item.project}/${item.dir}`;
  const {
    data: detail,
    error,
    reload,
  } = usePolling(() => api.detail(item.project, item.dir), 3000, key, onUnauthorized);
  const live = detail ?? item;
  /** The body the editor started from; set while the brief is being edited. */
  const [editing, setEditing] = useState<string>();
  useEffect(() => setEditing(undefined), [key]);
  const tab = routeTab ?? (live.session || isBlocked(live) ?"terminal" : "brief");
  const path = itemTerminal(item.project, item.dir);
  const popOut = `/?terminal=${encodeURIComponent(path)}`;
  const kindIcon = KIND_ICONS[live.kind.trim().toLowerCase()];

  const family = items.filter((i) => i.project === item.project);
  const parent = family.find((i) => i.dir === live.parent);
  const children = family.filter((i) => i.parent === item.dir);

  return (
    <aside className="detail">
      <header>
        <div>
          <h2>{live.title}</h2>
          <div className="meta">
            <span className="badge">{live.project}</span>
            <span className="badge">{stageLabel(live.stage)}</span>
            <span className="badge">{kindIcon && `${kindIcon} `}{live.kind}</span>
            <LinkBadges item={live} />
            <ReviewStrip review={live.review} />
            {isBlocked(live) && <span className="chip blocked">blocked</span>}
            {!!live.unpushed && <span className="chip unpushed" title="Commits in the worktree that are not on any remote">{live.unpushed} unpushed</span>}
            {live.pr_moved && <span className="chip pr-moved" title="The PR has commits this workitem has not seen">new commits</span>}
          </div>
          {(parent || children.length > 0) && (
            <div className="meta family">
              {parent && (
                <button className="link" onClick={() => onSelect(parent)}>
                  ↑ Parent: {parent.title}
                </button>
              )}
              {children.map((child) => (
                <button key={child.dir} className="link" onClick={() => onSelect(child)}>
                  ↳ {child.title} ({stageLabel(child.stage)})
                </button>
              ))}
            </div>
          )}
        </div>
        <button className="link" onClick={onClose}>
          <IconX />
          Close
        </button>
      </header>
      <nav className="tabs">
        <button className={tab === "brief" ? "active" : ""} onClick={() => onTab("brief")}>
          <IconFileText />
          Brief
        </button>
        <button className={tab === "info" ? "active" : ""} onClick={() => onTab("info")}>
          <IconInfo />
          Info
        </button>
        <button className={tab === "files" ? "active" : ""} onClick={() => onTab("files")}>
          <IconFolder />
          Files
        </button>
        {live.pr && (
          <button className={tab === "pr" ? "active" : ""} onClick={() => onTab("pr")}>
            <IconGitPullRequest />
            PR
          </button>
        )}
        <button className={tab === "terminal" ? "active" : ""} onClick={() => onTab("terminal")}>
          <IconTerminalSquare />
          Terminal
        </button>
        <button className={tab === "actions" ? "active" : ""} onClick={() => onTab("actions")}>
          <IconSliders />
          Actions
        </button>
        {live.session && (
          <a href={popOut} target="_blank" rel="noreferrer">
            <IconExternal />
            Pop out
          </a>
        )}
      </nav>
      {error && <p className="error">{error}</p>}
      {tab === "brief" && editing !== undefined && (
        <BriefEditor
          item={live}
          base={editing}
          onClose={(saved) => {
            setEditing(undefined);
            if (saved) void reload();
          }}
        />
      )}
      {tab === "brief" && editing === undefined && (
        <div className="brief">
          <BriefActions
            item={live}
            onDone={() => void reload()}
            onStarted={() => onTab("terminal")}
            onEdit={() => detail && setEditing(stripFrontmatter(detail.content))}
          />
          {detail ? <BriefView item={live} body={stripFrontmatter(detail.content)} onSaved={() => void reload()} /> : "Loading…"}
        </div>
      )}
      {tab === "info" && <div className="brief">{detail ? <Info item={live} content={detail.content} onSaved={() => void reload()} /> : "Loading…"}</div>}
      {tab === "files" && <Files item={live} onUnauthorized={onUnauthorized} />}
      {tab === "pr" && live.pr && (
        <div className="brief">
          <PrActions item={live} onDone={() => void reload()} />
          {live.review && live.review.failed.length > 0 && (
            <div className="failed-checks">
              <strong>Failing checks</strong>
              <ul>
                {live.review.failed.map((check) => (
                  <li key={check.name}>
                    {check.url ? (
                      <a href={check.url} target="_blank" rel="noreferrer">
                        {check.name} ↗
                      </a>
                    ) : (
                      check.name
                    )}
                  </li>
                ))}
              </ul>
            </div>
          )}
          <PrConversation item={live} onUnauthorized={onUnauthorized} />
        </div>
      )}
      {tab === "terminal" && (
        <>
          <AgentActions item={live} onDone={() => void reload()} />
          {live.session && <Terminal path={path} />}
        </>
      )}
      {tab === "actions" && (
        <Actions item={live} onDone={() => void reload()} />
      )}
    </aside>
  );
}
