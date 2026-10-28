import { KeyboardEvent, useEffect, useRef, useState } from "react";
import { prNumber, searchItems, stageLabel, ticket } from "./boardState";
import { Dialog } from "./Dialog";
import { Item } from "./types";

const LIMIT = 20;

type Props = { items: Item[]; showProject: boolean; onOpen: (item: Item) => void; onClose: () => void };

/** Finds a workitem by title, Jira key or PR number. Arrows pick a result, Enter opens it. */
export function QuickSearch({ items, showProject, onOpen, onClose }: Props) {
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const list = useRef<HTMLUListElement>(null);
  const found = searchItems(items, query).slice(0, LIMIT);
  const current = Math.min(active, found.length - 1);

  useEffect(() => {
    list.current?.children[current]?.scrollIntoView({ block: "nearest" });
  }, [current]);

  const onKey = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      setActive((current + step + found.length) % Math.max(found.length, 1));
    } else if (event.key === "Enter" && found[current]) {
      event.preventDefault();
      onOpen(found[current]);
    }
  };

  return (
    <Dialog title="Find workitem" onClose={onClose}>
      <input
        className="quick-search-input"
        autoFocus
        placeholder="Title, Jira key or PR number"
        aria-label="Search workitems"
        role="combobox"
        aria-expanded={found.length > 0}
        aria-controls="quick-search-results"
        aria-activedescendant={found[current] ? `quick-search-${current}` : undefined}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setActive(0);
        }}
        onKeyDown={onKey}
      />
      {query.trim() && !found.length && <p className="quick-search-empty">No matching workitem.</p>}
      {found.length > 0 && (
        <ul className="quick-search-results" id="quick-search-results" role="listbox" ref={list}>
          {found.map((item, index) => {
            const pr = prNumber(item.pr);
            return (
              <li
                key={`${item.project}/${item.dir}`}
                id={`quick-search-${index}`}
                role="option"
                aria-selected={index === current}
                className={index === current ? "active" : ""}
                onMouseMove={() => setActive(index)}
                onClick={() => onOpen(item)}
              >
                <span className="quick-search-title">{item.title}</span>
                <span className="quick-search-meta">
                  {showProject && <span className="badge">{item.project}</span>}
                  <span className="badge">{stageLabel(item.stage)}</span>
                  {ticket(item.issue) && <span className="badge">{ticket(item.issue)!.key}</span>}
                  {pr && <span className="badge">PR #{pr}</span>}
                </span>
              </li>
            );
          })}
        </ul>
      )}
    </Dialog>
  );
}
