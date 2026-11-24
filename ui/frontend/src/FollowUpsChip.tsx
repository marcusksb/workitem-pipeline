import { Badge } from "@/components/ui/badge";

/** Count of the item's open follow-ups; the tooltip lists them. A button when `onClick` is set. */
export function FollowUpsChip({ followUps, onClick }: { followUps: string[]; onClick?: () => void }) {
  if (followUps.length === 0) return null;
  const label = `${followUps.length} follow-up${followUps.length === 1 ? "" : "s"}`;
  const title = followUps.map((f) => `• ${f}`).join("\n");
  return onClick ? (
    <Badge asChild variant="wait" className="chip follow-ups cursor-pointer hover:bg-working/10">
      <button title={title} onClick={onClick}>
        {label}
      </button>
    </Badge>
  ) : (
    <Badge variant="wait" className="chip follow-ups" title={title}>
      {label}
    </Badge>
  );
}
