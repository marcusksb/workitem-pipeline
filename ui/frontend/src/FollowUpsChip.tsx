/** Count of the item's open follow-ups; the tooltip lists them. A button when `onClick` is set. */
export function FollowUpsChip({ followUps, onClick }: { followUps: string[]; onClick?: () => void }) {
  if (followUps.length === 0) return null;
  const label = `${followUps.length} follow-up${followUps.length === 1 ? "" : "s"}`;
  const title = followUps.map((f) => `• ${f}`).join("\n");
  return onClick ? (
    <button className="chip follow-ups" title={title} onClick={onClick}>
      {label}
    </button>
  ) : (
    <div className="chip follow-ups" title={title}>
      {label}
    </div>
  );
}
