import { agentTerminal } from "./api";
import { stageLabel } from "./boardState";
import { InboxEntry } from "./inboxState";
import { Item, Plan } from "./types";

type Props = {
  groups: [string, InboxEntry[]][];
  onItem: (item: Item) => void;
  onPlan: (plan: Plan) => void;
};

/** What needs you across every workspace (decision 60), most urgent first. */
export function Inbox({ groups, onItem, onPlan }: Props) {
  if (groups.length === 0) return <p className="empty">Nothing needs you right now.</p>;
  return (
    <div className="inbox">
      {groups.map(([workspace, entries]) => (
        <section key={workspace}>
          <h2>{workspace}</h2>
          <ul>
            {entries.map((entry) => (
              <li key={key(entry)}>
                <Row entry={entry} onItem={onItem} onPlan={onPlan} />
              </li>
            ))}
          </ul>
        </section>
      ))}
    </div>
  );
}

const key = (entry: InboxEntry) =>
  entry.kind === "item" ? `item/${entry.item.dir}` : entry.kind === "plan" ? `plan/${entry.plan.id}` : `session/${entry.session.name}`;

function Row({ entry, onItem, onPlan }: { entry: InboxEntry } & Omit<Props, "groups">) {
  const reason = <span className={`inbox-reason rank-${entry.rank}`}>{entry.reason}</span>;
  if (entry.kind === "item") {
    return (
      <button className="inbox-row" onClick={() => onItem(entry.item)}>
        <span className="inbox-title">{entry.item.title}</span>
        <span className="inbox-meta">
          {reason}
          <span className="badge">{stageLabel(entry.item.stage)}</span>
        </span>
      </button>
    );
  }
  if (entry.kind === "plan") {
    return (
      <button className="inbox-row" onClick={() => onPlan(entry.plan)}>
        <span className="inbox-title">{entry.plan.topic || entry.plan.id}</span>
        <span className="inbox-meta">{reason}</span>
      </button>
    );
  }
  // A create session has no workitem and no transcript yet, so only its terminal can answer it.
  return (
    <a className="inbox-row" href={`/?terminal=${encodeURIComponent(agentTerminal(entry.session.workspace, entry.session.name))}`} target="_blank" rel="noreferrer">
      <span className="inbox-title">{entry.session.name}</span>
      <span className="inbox-meta">{reason}</span>
    </a>
  );
}
