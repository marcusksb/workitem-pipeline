import { waitingFor } from "./boardState";
import { IconSnooze } from "./icons";
import { Item } from "./types";

export function WaitingChip({ item }: { item: Item }) {
  return (
    <span className="chip waiting" title={waitingFor(item) || "waiting"}>
      <IconSnooze /> waiting
    </span>
  );
}
