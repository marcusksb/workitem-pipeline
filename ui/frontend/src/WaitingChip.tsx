import { waitingFor } from "./boardState";
import { IconSnooze } from "./icons";
import { Item } from "./types";
import { Badge } from "@/components/ui/badge";

export function WaitingChip({ item }: { item: Item }) {
  return (
    <Badge title={waitingFor(item) || "waiting"}>
      <IconSnooze /> waiting
    </Badge>
  );
}
